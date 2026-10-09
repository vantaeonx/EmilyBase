use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn stop(message: &str) {
    println!("{message}");
    std::io::stdout().flush().unwrap();
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
}

#[test]
#[ignore = "temporary bounded native put fixture invoked explicitly by parent"]
fn bounded_worker() {
    let path = std::env::var_os("EMILYBASE_BOUNDED_DIRECTORY").unwrap();
    let phase = std::env::var("EMILYBASE_BOUNDED_PHASE").unwrap();
    let mut owner = ProjectDirectory::initialize(path, PROJECT).unwrap();
    let payload = if phase == "empty" {
        &[][..]
    } else {
        &b"full"[..]
    };
    let result = owner
        .put_bounded_with(
            OBJECT,
            payload,
            WriteLimits::new(1, 4).unwrap(),
            || {
                if phase == "prepared" {
                    stop("BOUNDED_PREPARED");
                }
            },
            || {
                if phase == "selected" {
                    stop("BOUNDED_SELECTED");
                }
            },
        )
        .unwrap();
    assert_eq!(result.inventory().entries().len(), 1);
    assert_eq!(result.inventory().payload_bytes(), payload.len() as u64);
    stop("BOUNDED_ACK");
}
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn actual_kills_preserve_received_bounded_writes_and_recompute_capacity_after_restart() {
    for phase in ["prepared", "selected", "empty", "full"] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "directory::bounded::crash_tests::bounded_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_BOUNDED_DIRECTORY", &path)
                .env("EMILYBASE_BOUNDED_PHASE", phase)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = worker.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if line.starts_with("BOUNDED_") {
                    let _ = sender.send(line);
                    break;
                }
            }
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(15)).unwrap(),
            match phase {
                "prepared" => "BOUNDED_PREPARED",
                "selected" => "BOUNDED_SELECTED",
                _ => "BOUNDED_ACK",
            }
        );
        assert!(matches!(
            ProjectDirectory::open(&path, PROJECT),
            Err(Error::Busy)
        ));
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        let mut owner = ProjectDirectory::open(&path, PROJECT).unwrap();
        let limits = WriteLimits::new(1, 4).unwrap();
        if phase == "prepared" {
            assert_eq!(owner.inventory().unwrap().entries().len(), 0);
            assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
            owner.put_bounded(OBJECT, b"full", limits).unwrap();
        } else {
            assert!(matches!(
                owner.put_bounded(OBJECT, b"full", limits),
                Err(Error::Exists)
            ));
        }
        assert_eq!(
            owner.get(OBJECT).unwrap().payload(),
            if phase == "empty" {
                &[][..]
            } else {
                &b"full"[..]
            }
        );
        assert!(matches!(
            owner.put_bounded(ObjectId::from_bytes([4; 16]), &[], limits),
            Err(Error::Limit)
        ));
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 2);
    }
}
