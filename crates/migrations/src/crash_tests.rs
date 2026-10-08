use super::*;
use emilybase_catalog::Key;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

const FIRST: &str = "CREATE TABLE t(id INT PRIMARY KEY,value INT); INSERT INTO t VALUES(0,0)";
const SECOND: &str = "CREATE TABLE replacement(id INT PRIMARY KEY,value INT,extra TEXT); \
INSERT INTO replacement(id,value) SELECT * FROM t; UPDATE replacement SET value=9; DROP TABLE t; \
CREATE TABLE t(id INT PRIMARY KEY,value INT,extra TEXT); INSERT INTO t SELECT * FROM replacement; \
DROP TABLE replacement; CREATE TABLE next(id INT PRIMARY KEY)";

#[test]
fn killed_first_and_next_migrations_never_separate_schema_from_receipts() {
    for format in [1, 2] {
        for version in [1, 2] {
            for phase in ["staged", "ack"] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("db");
                let mut database = Database::create(&path).unwrap();
                if format == 2 {
                    database.compact().unwrap();
                }
                if version == 2 {
                    apply(&mut database, &prepare(1, "first", FIRST).unwrap()).unwrap();
                }
                let base = database.last_transaction();
                let before = database.committed_wal().unwrap();
                drop(database);
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "crash_tests::migration_writer_helper",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_MIGRATION_PATH", &path)
                    .env("EMILYBASE_MIGRATION_VERSION", version.to_string())
                    .env("EMILYBASE_MIGRATION_PHASE", phase)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (sender, receiver) = std::sync::mpsc::channel();
                let reader = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        match line {
                            Ok(line) if line.starts_with("MIGRATION_READY ") => {
                                let _ = sender.send(line);
                                break;
                            }
                            Err(_) => break,
                            _ => {}
                        }
                    }
                });
                let marker = receiver.recv_timeout(Duration::from_secs(20));
                let killed = child.kill();
                let status = child.wait().unwrap();
                reader.join().unwrap();
                assert!(killed.is_ok());
                assert!(!status.success());
                assert_eq!(marker.unwrap(), format!("MIGRATION_READY {phase}"));
                let mut database = Database::open(&path).unwrap();
                let committed = phase == "ack";
                assert_eq!(database.last_transaction(), base + u64::from(committed));
                assert_eq!(
                    inspect(&database).unwrap().len(),
                    version as usize - 1 + usize::from(committed)
                );
                if version == 1 {
                    assert_eq!(database.view().unwrap().schema("t").is_ok(), committed);
                    assert_eq!(
                        database.view().unwrap().schema(LEDGER_TABLE).is_ok(),
                        committed
                    );
                } else {
                    assert_eq!(
                        database
                            .view()
                            .unwrap()
                            .get("t", &Key::Integer(0))
                            .unwrap()
                            .unwrap()[1],
                        Value::Integer(if committed { 9 } else { 0 })
                    );
                    assert_eq!(database.view().unwrap().schema("next").is_ok(), committed);
                    assert_eq!(
                        database.view().unwrap().schema("t").unwrap().columns.len(),
                        if committed { 3 } else { 2 }
                    );
                }
                if !committed {
                    assert_eq!(database.committed_wal().unwrap(), before);
                }
                let migration = prepare(
                    version,
                    if version == 1 { "first" } else { "second" },
                    if version == 1 { FIRST } else { SECOND },
                )
                .unwrap();
                let applied = apply(&mut database, &migration).unwrap();
                assert_eq!(applied.already_applied, committed);
                assert_eq!(inspect(&database).unwrap().len(), version as usize);
                assert_eq!(database.last_transaction(), base + 1);
            }
        }
    }
}

#[test]
#[ignore = "subprocess helper, invoked by its parent kill test"]
fn migration_writer_helper() {
    let path = std::env::var_os("EMILYBASE_MIGRATION_PATH").unwrap();
    let version = std::env::var("EMILYBASE_MIGRATION_VERSION")
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let phase = std::env::var("EMILYBASE_MIGRATION_PHASE").unwrap();
    let mut database = Database::open(path).unwrap();
    let migration = prepare(
        version,
        if version == 1 { "first" } else { "second" },
        if version == 1 { FIRST } else { SECOND },
    )
    .unwrap();
    let report = apply_inner(&mut database, &migration, || {
        if phase == "staged" {
            ready("staged");
        }
    })
    .unwrap();
    assert!(!report.already_applied);
    ready("ack");
}

fn ready(phase: &str) -> ! {
    println!("MIGRATION_READY {phase}");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
