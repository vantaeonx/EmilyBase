use emilybase_storage::Page;
use emilybase_wal::{Error, Wal};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
const ID: [u8; 16] = [7; 16];
fn page(id: u64, text: &[u8]) -> Page {
    let mut page = Page::new(id).unwrap();
    page.insert(text).unwrap();
    page
}
struct Worker(Child);

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "subprocess helper, invoked by its parent test"]
fn crash_worker() {
    let Ok(path) = std::env::var("EMILYBASE_WAL_TEST_PATH") else {
        return;
    };
    let (mut wal, _) = Wal::open(&path, Some(ID)).unwrap();
    if std::env::var("EMILYBASE_WAL_TEST_PHASE").unwrap() == "committed" {
        wal.append(&[page(1, b"survives forced termination")])
            .unwrap();
    } else {
        let mut pending = wal.begin(&[page(1, b"must never appear")]).unwrap();
        pending.sync_uncommitted().unwrap();
    }
    println!("READY");
    std::io::stdout().flush().unwrap();
    // Parent kills this process while ownership is held; no Rust destructors run.
    let _ = std::io::stdin().read(&mut [0u8; 1]);
}

#[test]
fn process_kill_preserves_acknowledged_and_excludes_uncommitted_batches() {
    for phase in ["committed", "pending"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redo.wal");
        let mut wal = Wal::create(&path, ID).unwrap();
        wal.append(&[page(1, b"baseline")]).unwrap();
        drop(wal);
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_worker", "--nocapture", "--ignored"])
                .env("EMILYBASE_WAL_TEST_PATH", &path)
                .env("EMILYBASE_WAL_TEST_PHASE", phase)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = worker.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line == "READY" {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(Wal::open(&path, None), Err(Error::Busy)));
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        let (_, recovery) = Wal::open(path, Some(ID)).unwrap();
        let count = if phase == "committed" { 2 } else { 1 };
        assert_eq!(recovery.committed.len(), count);
        let expected: &[u8] = if phase == "committed" {
            b"survives forced termination"
        } else {
            b"baseline"
        };
        assert_eq!(
            recovery.committed.last().unwrap().pages[0].get(0).unwrap(),
            expected
        );
    }
}
