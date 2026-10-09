use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);

#[test]
#[ignore = "invoked by the parent with a temporary synthetic path"]
fn publication_worker() {
    let target = std::env::var_os("EMILYBASE_OBJECT_CREATE_TARGET").unwrap();
    let payload = vec![0x59; 128 * 1024];
    let report = publish_file(target, PROJECT, OBJECT, &payload).unwrap();
    assert_eq!(report.payload_bytes, payload.len());
    println!("OBJECT_PUBLICATION_ACK");
    std::io::stdout().flush().unwrap();
    let mut input = [0];
    std::io::stdin().read_exact(&mut input).unwrap();
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn received_native_object_publication_survives_process_kill_and_reinspection() {
    for _ in 0..2 {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("selected.object");
        let expected_payload = vec![0x59; 128 * 1024];
        let expected = encode(PROJECT, OBJECT, &expected_payload).unwrap();
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "crash_tests::publication_worker",
                    "--nocapture",
                    "--ignored",
                ])
                .env("EMILYBASE_OBJECT_CREATE_TARGET", &target)
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
                if line == "OBJECT_PUBLICATION_ACK" {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), expected);
        let report = inspect_file(&target, PROJECT, OBJECT).unwrap();
        assert_eq!(report.payload_bytes, expected_payload.len());
        assert!(inspect_file(&target, ProjectId::from_bytes([3; 16]), OBJECT).is_err());
        assert!(publish_file(&target, PROJECT, OBJECT, b"replacement").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), expected);
        assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 1);
    }
}
