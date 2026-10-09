use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);

#[test]
#[ignore = "temporary native directory fixture invoked explicitly by parent"]
fn directory_worker() {
    let path = std::env::var_os("EMILYBASE_OBJECT_DIRECTORY").unwrap();
    let phase = std::env::var("EMILYBASE_OBJECT_DIRECTORY_PHASE").unwrap();
    let mut owner = ProjectDirectory::initialize(path, PROJECT).unwrap();
    if phase != "initialized" {
        let payload = if phase == "empty" {
            Vec::new()
        } else {
            vec![0x59; 128 * 1024]
        };
        owner.put(OBJECT, &payload).unwrap();
        assert_eq!(owner.get(OBJECT).unwrap().payload(), payload);
    }
    println!("OBJECT_DIRECTORY_ACK");
    std::io::stdout().flush().unwrap();
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
}
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn received_directory_and_object_results_survive_kill_and_release_exclusive_owner() {
    for phase in ["initialized", "empty", "full"] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("objects");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "directory_crash_tests::directory_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_OBJECT_DIRECTORY", &path)
                .env("EMILYBASE_OBJECT_DIRECTORY_PHASE", phase)
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
                if line == "OBJECT_DIRECTORY_ACK" {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(
            ProjectDirectory::open(&path, PROJECT),
            Err(Error::Busy)
        ));
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        let marker = std::fs::read(path.join(".emilybase-objects")).unwrap();
        assert_eq!(
            marker,
            encode(PROJECT, ObjectId::from_bytes([0; 16]), &[]).unwrap()
        );
        let mut owner = ProjectDirectory::open(&path, PROJECT).unwrap();
        if phase == "initialized" {
            assert!(owner.get(OBJECT).is_err());
            assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
            owner.put(OBJECT, b"synthetic-new-after-kill").unwrap();
        } else {
            let payload = if phase == "empty" {
                Vec::new()
            } else {
                vec![0x59; 128 * 1024]
            };
            assert_eq!(owner.get(OBJECT).unwrap().payload(), payload);
            let before = std::fs::read(path.join(format!("{OBJECT}.object"))).unwrap();
            assert_eq!(before, encode(PROJECT, OBJECT, &payload).unwrap());
            assert!(owner.put(OBJECT, b"replacement").is_err());
            assert_eq!(
                std::fs::read(path.join(format!("{OBJECT}.object"))).unwrap(),
                before
            );
            assert_eq!(std::fs::read_dir(&path).unwrap().count(), 2);
        }
    }
}
