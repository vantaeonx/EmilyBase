//! Test-only publication boundaries; none are present in release builds.
use crate::{Error, ProjectStore};
use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// fork briefly inherits unrelated open file descriptions until exec. Serialize
// filesystem unit tests around subprocess launches; keep engine ownership strict.
pub(crate) static PROCESS_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

thread_local! {
    static FAULT: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}
pub(crate) fn fail(boundary: &str) -> std::io::Result<()> {
    FAULT.with_borrow_mut(|selected| {
        if selected.as_ref().is_some_and(|value| *value == boundary) {
            *selected = None;
            Err(std::io::Error::other("injected publication sync failure"))
        } else {
            Ok(())
        }
    })
}
pub(crate) fn checkpoint(boundary: &str) {
    if std::env::var("EMILYBASE_REGISTRY_KILL_POINT").as_deref() == Ok(boundary) {
        println!("REGISTRY_BOUNDARY {boundary}");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
fn inject(boundary: &'static str) {
    FAULT.with_borrow_mut(|value| *value = Some(boundary));
}
fn seed(path: &Path) -> (String, String) {
    let mut store = ProjectStore::open(path).unwrap();
    let created = store.create("existing").unwrap();
    store.authorize(&created.project.id,&created.api_key).unwrap()
        .execute("CREATE TABLE original(id INT PRIMARY KEY,text TEXT); INSERT INTO original VALUES (1,'synthetic')",&[]).unwrap();
    (created.project.id, created.api_key)
}
fn original(store: &ProjectStore, id: &str, key: &str) {
    let report = store
        .authorize(id, key)
        .unwrap()
        .execute("SELECT * FROM original", &[])
        .unwrap();
    assert_eq!(report.transaction, 2);
    assert_eq!(report.results[0].rows.len(), 1);
    assert_eq!(
        report.results[0].rows[0][1],
        emilybase_catalog::Value::Text("synthetic".into())
    );
}
fn kill_at(root: &Path, id: &str, action: &str, point: &str, credential_file: &Path) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "durability::publication_helper",
            "--nocapture",
        ])
        .env("EMILYBASE_REGISTRY_KILL_ROOT", root)
        .env("EMILYBASE_REGISTRY_KILL_ID", id)
        .env("EMILYBASE_REGISTRY_KILL_ACTION", action)
        .env("EMILYBASE_REGISTRY_KILL_POINT", point)
        .env("EMILYBASE_REGISTRY_KEY_FILE", credential_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let reached = loop {
        match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(line)) if line == format!("REGISTRY_BOUNDARY {point}") => break true,
            Ok(Ok(_)) => (),
            _ => break false,
        }
    };
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    drop(receive);
    reader.join().unwrap();
    assert!(reached, "publication boundary was not reached: {point}");
}
#[test]
fn create_kill_boundaries_never_adopt_incomplete_projects_and_preserve_existing_rows() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for point in [
        "create_data_synced",
        "create_metadata_synced",
        "create_stage_synced",
        "create_renamed",
        "create_root_synced",
        "create_ack",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let (id, key) = seed(&root);
        let prior = fs::read(root.join(&id).join("data/redo.wal")).unwrap();
        let key_file = temp.path().join("private-key");
        kill_at(&root, &id, "create", point, &key_file);
        let mut store = ProjectStore::open(&root).unwrap();
        original(&store, &id, &key);
        assert_eq!(
            fs::read(root.join(&id).join("data/redo.wal")).unwrap(),
            prior
        );
        let published = matches!(
            point,
            "create_renamed" | "create_root_synced" | "create_ack"
        );
        let projects = store.list().unwrap();
        assert_eq!(projects.len(), if published { 2 } else { 1 });
        if published {
            let added = projects.iter().find(|project| project.id != id).unwrap();
            assert_eq!(added.name, "new-project");
            assert_eq!(added.key_epoch, 1);
            if point == "create_ack" {
                let returned_key = fs::read_to_string(&key_file).unwrap();
                assert_eq!(
                    store
                        .authorize(&added.id, &returned_key)
                        .unwrap()
                        .status()
                        .unwrap()
                        .tables,
                    0
                );
            }
            let rotated = store.rotate(&added.id).unwrap();
            assert_eq!(rotated.project.key_epoch, 2);
            assert_eq!(
                store
                    .authorize(&added.id, &rotated.api_key)
                    .unwrap()
                    .status()
                    .unwrap()
                    .transaction,
                1
            );
        } else {
            assert!(fs::read_dir(&root).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".creating-")
            }));
        }
        assert_eq!(store.create("after-restart").unwrap().project.key_epoch, 1);
    }
}
#[test]
fn rotation_kills_publish_one_complete_epoch_and_never_change_database_bytes() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for point in [
            "rotate_file_synced",
            "rotate_renamed",
            "rotate_directory_synced",
            "rotate_ack",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("projects");
            let (id, key) = seed(&root);
            let data = root.join(&id).join("data");
            if compact {
                emilybase_transactions::Database::open(&data)
                    .unwrap()
                    .compact()
                    .unwrap();
            }
            let prior = fs::read(data.join("redo.wal")).unwrap();
            let key_file = temp.path().join("private-key");
            kill_at(&root, &id, "rotate", point, &key_file);
            let mut store = ProjectStore::open(&root).unwrap();
            let published = point != "rotate_file_synced";
            assert_eq!(
                store.list().unwrap()[0].key_epoch,
                if published { 2 } else { 1 }
            );
            assert_eq!(fs::read(data.join("redo.wal")).unwrap(), prior);
            if published {
                assert!(matches!(store.authorize(&id, &key), Err(Error::Denied)));
            } else {
                original(&store, &id, &key);
            }
            if point == "rotate_ack" {
                original(&store, &id, &fs::read_to_string(&key_file).unwrap());
            }
            let replacement = store.rotate(&id).unwrap();
            assert_eq!(replacement.project.key_epoch, if published { 3 } else { 2 });
            original(&store, &id, &replacement.api_key);
        }
    }
}
#[test]
fn create_sync_failures_before_and_after_rename_have_distinct_safe_outcomes() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for boundary in ["create_data_sync", "create_stage_sync", "create_root_sync"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let (id, key) = seed(&root);
        let mut store = ProjectStore::open(&root).unwrap();
        inject(boundary);
        let outcome = store.create("faulted");
        if boundary == "create_root_sync" {
            assert!(matches!(outcome, Err(Error::PublicationUnknown(_))));
            assert!(matches!(store.list(), Err(Error::Poisoned)));
            assert!(matches!(store.create("no retry"), Err(Error::Poisoned)));
            assert!(matches!(store.authorize(&id, &key), Err(Error::Poisoned)));
        } else {
            assert!(matches!(outcome, Err(Error::Io(_))));
            assert_eq!(store.list().unwrap().len(), 1);
            original(&store, &id, &key);
        }
        drop(store);
        let mut reopened = ProjectStore::open(&root).unwrap();
        assert_eq!(
            reopened.list().unwrap().len(),
            if boundary == "create_root_sync" { 2 } else { 1 }
        );
        original(&reopened, &id, &key);
        reopened.create("recovered").unwrap();
    }
}
#[test]
fn rotation_sync_failure_poisoning_preserves_rows_and_allows_privileged_recovery() {
    let _serial = PROCESS_TESTS.blocking_lock();
    for boundary in ["rotate_file_sync", "rotate_directory_sync"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let (id, key) = seed(&root);
        let mut store = ProjectStore::open(&root).unwrap();
        let accepted = store.authorize(&id, &key).unwrap();
        let prior = fs::read(root.join(&id).join("project.json")).unwrap();
        inject(boundary);
        let outcome = store.rotate(&id);
        if boundary == "rotate_directory_sync" {
            assert!(matches!(outcome, Err(Error::PublicationUnknown(_))));
            assert!(matches!(store.list(), Err(Error::Poisoned)));
            assert!(matches!(store.rotate(&id), Err(Error::Poisoned)));
        } else {
            assert!(matches!(outcome, Err(Error::Io(_))));
            assert_eq!(
                fs::read(root.join(&id).join("project.json")).unwrap(),
                prior
            );
            original(&store, &id, &key);
        }
        assert_eq!(accepted.status().unwrap().rows, 1);
        drop(store);
        let mut reopened = ProjectStore::open(root).unwrap();
        let replacement = reopened.rotate(&id).unwrap();
        assert_eq!(
            replacement.project.key_epoch,
            if boundary == "rotate_directory_sync" {
                3
            } else {
                2
            }
        );
        original(&reopened, &id, &replacement.api_key);
    }
}
#[test]
#[ignore = "subprocess boundary helper invoked by parent tests"]
fn publication_helper() {
    let root = std::env::var_os("EMILYBASE_REGISTRY_KILL_ROOT").unwrap();
    let mut store = ProjectStore::open(root).unwrap();
    let action = std::env::var("EMILYBASE_REGISTRY_KILL_ACTION").unwrap();
    let response = if action == "create" {
        store.create("new-project").unwrap()
    } else {
        store
            .rotate(&std::env::var("EMILYBASE_REGISTRY_KILL_ID").unwrap())
            .unwrap()
    };
    let key_path = std::env::var_os("EMILYBASE_REGISTRY_KEY_FILE").unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(key_path)
        .unwrap();
    file.write_all(response.api_key.as_bytes()).unwrap();
    file.sync_all().unwrap();
    checkpoint(if action == "create" {
        "create_ack"
    } else {
        "rotate_ack"
    });
}
