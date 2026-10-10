use super::*;
use crate::encode_archive;
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
#[ignore = "temporary native archive fixture invoked explicitly by parent"]
fn archive_worker() {
    let source = std::env::var_os("EMILYBASE_ARCHIVE_SOURCE").unwrap();
    let path = std::env::var_os("EMILYBASE_ARCHIVE_TARGET").unwrap();
    let phase = std::env::var("EMILYBASE_ARCHIVE_PHASE").unwrap();
    let mut owner = ProjectDirectory::initialize(source, PROJECT).unwrap();
    if phase != "empty" {
        owner.put(OBJECT, &vec![0x59; 128 * 1024]).unwrap();
    }
    let result = owner
        .backup_with(
            Path::new(&path),
            || {
                if phase == "captured" {
                    stop("ARCHIVE_CAPTURED");
                }
            },
            || {
                if phase == "selected" {
                    stop("ARCHIVE_SELECTED");
                }
            },
        )
        .unwrap();
    assert_eq!(result.objects, usize::from(phase != "empty"));
    stop("ARCHIVE_ACK");
}
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn actual_kills_preserve_received_archives_and_distinguish_preselection_from_unreceived_result() {
    for phase in ["captured", "selected", "empty", "full"] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&source)
            .unwrap();
        let path = temp.path().join("copy");
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "directory::backup::crash_tests::archive_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_ARCHIVE_SOURCE", &source)
                .env("EMILYBASE_ARCHIVE_TARGET", &path)
                .env("EMILYBASE_ARCHIVE_PHASE", phase)
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
                if line.starts_with("ARCHIVE_") {
                    let _ = sender.send(line);
                    break;
                }
            }
        });
        let message = receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        assert_eq!(
            message,
            match phase {
                "captured" => "ARCHIVE_CAPTURED",
                "selected" => "ARCHIVE_SELECTED",
                _ => "ARCHIVE_ACK",
            }
        );
        assert!(matches!(
            ProjectDirectory::open(&source, PROJECT),
            Err(Error::Busy)
        ));
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        let owner = ProjectDirectory::open(&source, PROJECT).unwrap();
        let expected = encode_archive(&owner.capture().unwrap()).unwrap();
        if phase == "captured" {
            assert!(!path.exists());
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
            owner.backup_to(&path).unwrap();
        } else {
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
            assert!(owner.backup_to(&path).is_err());
        }
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        let report = crate::inspect_archive_file(&path, PROJECT).unwrap();
        assert_eq!(report.objects, usize::from(phase != "empty"));
        assert_eq!(
            report.payload_bytes,
            if phase == "empty" { 0 } else { 128 * 1024 }
        );
        assert_eq!(report.digest, *owner.inventory().unwrap().digest());
        if phase != "empty" {
            assert_eq!(owner.get(OBJECT).unwrap().payload(), vec![0x59; 128 * 1024]);
        }
    }
}
