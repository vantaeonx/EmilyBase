use super::*;
use crate::{ObjectId, encode_archive};
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
#[ignore = "temporary native object restore fixture invoked explicitly by parent"]
fn restore_worker() {
    let source = std::env::var_os("EMILYBASE_RESTORE_ARCHIVE").unwrap();
    let target = std::env::var_os("EMILYBASE_RESTORE_DIRECTORY").unwrap();
    let phase = std::env::var("EMILYBASE_RESTORE_PHASE").unwrap();
    let result = restore_file_with(
        Path::new(&source),
        PROJECT,
        Path::new(&target),
        || {
            if phase == "populated" {
                stop("RESTORE_POPULATED");
            }
        },
        || {
            if phase == "selected" {
                stop("RESTORE_SELECTED");
            }
        },
    )
    .unwrap();
    assert_eq!(result.objects, usize::from(phase != "empty"));
    stop("RESTORE_ACK");
}
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn actual_restore_kills_preserve_unselected_private_stage_or_exact_selected_received_directory() {
    for phase in ["populated", "selected", "empty", "full"] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&source)
            .unwrap();
        let mut owner = ProjectDirectory::initialize(&source, PROJECT).unwrap();
        if phase != "empty" {
            owner.put(OBJECT, &vec![0x59; 128 * 1024]).unwrap();
        }
        let bytes = encode_archive(&owner.capture().unwrap()).unwrap();
        drop(owner);
        let archive = temp.path().join("source.object-archive");
        emilybase_storage::publish_private_file(&archive, &bytes, MAX_ARCHIVE_BYTES).unwrap();
        let target = temp.path().join("restored");
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "restore::crash_tests::restore_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_RESTORE_ARCHIVE", &archive)
                .env("EMILYBASE_RESTORE_DIRECTORY", &target)
                .env("EMILYBASE_RESTORE_PHASE", phase)
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
                if line.starts_with("RESTORE_") {
                    let _ = sender.send(line);
                    break;
                }
            }
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(15)).unwrap(),
            match phase {
                "populated" => "RESTORE_POPULATED",
                "selected" => "RESTORE_SELECTED",
                _ => "RESTORE_ACK",
            }
        );
        worker.0.kill().unwrap();
        assert!(!worker.0.wait().unwrap().success());
        reader.join().unwrap();
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        if phase == "populated" {
            assert!(!target.exists());
            let partial = std::fs::read_dir(temp.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| {
                    p.file_name()
                        .unwrap()
                        .as_encoded_bytes()
                        .starts_with(b".emilybase-directory-")
                })
                .unwrap();
            let owner = ProjectDirectory::open(&partial, PROJECT).unwrap();
            assert_eq!(encode_archive(&owner.capture().unwrap()).unwrap(), bytes);
            drop(owner);
            restore_archive_file(&archive, PROJECT, &target).unwrap();
            assert!(partial.exists());
        } else {
            assert!(restore_archive_file(&archive, PROJECT, &target).is_err());
        }
        let owner = ProjectDirectory::open(&target, PROJECT).unwrap();
        assert_eq!(encode_archive(&owner.capture().unwrap()).unwrap(), bytes);
        assert_eq!(
            owner.inventory().unwrap().entries().len(),
            usize::from(phase != "empty")
        );
    }
}
