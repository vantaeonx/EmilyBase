use super::*;
use std::io::{BufRead, BufReader, Cursor};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn stop(message: &str) {
    println!("{message}");
    io::stdout().flush().unwrap();
    io::stdin().read_exact(&mut [0]).unwrap();
}
struct Source {
    bytes: Cursor<Vec<u8>>,
    stop_copy: bool,
}
impl Read for Source {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.stop_copy && self.bytes.position() >= 8192 {
            self.stop_copy = false;
            stop("STREAM_COPY");
        }
        self.bytes.read(out)
    }
}
impl Seek for Source {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.bytes.seek(from)
    }
}

#[test]
#[ignore = "temporary reader publication fixture invoked explicitly by parent"]
fn stream_worker() {
    let directory = File::open(std::env::var_os("EMILYBASE_STREAM_PARENT").unwrap()).unwrap();
    let phase = std::env::var("EMILYBASE_STREAM_PHASE").unwrap();
    let length = if phase == "empty" { 0 } else { 20000 };
    let mut source = Source {
        bytes: Cursor::new(vec![0x59; length]),
        stop_copy: phase == "copy",
    };
    let file = initialize(
        Pending::at(&directory, "selected".as_ref()).unwrap(),
        &mut source,
        length,
        || {
            if phase == "synced" {
                stop("STREAM_SYNCED");
            }
        },
        || {
            if phase == "selected" {
                stop("STREAM_SELECTED");
            }
        },
    )
    .unwrap();
    assert_eq!(file.metadata().unwrap().len(), length as u64);
    stop("STREAM_ACK");
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn actual_process_kills_distinguish_unselected_stages_selected_uncertainty_and_received_success() {
    // Match the existing creation subprocess fixture: process spawning must not
    // overlap immediate lock-release/reopen assertions on other test threads.
    // A fork may briefly retain their open file descriptions until exec closes them.
    let _serial = crate::publication_tests::CASES.lock().unwrap();
    for phase in ["copy", "synced", "selected", "full", "empty"] {
        let temp = tempfile::tempdir().unwrap();
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "stream_file::crash_tests::stream_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EMILYBASE_STREAM_PARENT", temp.path())
                .env("EMILYBASE_STREAM_PHASE", phase)
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
                if line.starts_with("STREAM_") {
                    let _ = sender.send(line);
                    break;
                }
            }
        });
        let expected = match phase {
            "copy" => "STREAM_COPY",
            "synced" => "STREAM_SYNCED",
            "selected" => "STREAM_SELECTED",
            _ => "STREAM_ACK",
        };
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
            expected
        );
        worker.0.kill().unwrap();
        worker.0.wait().unwrap();
        reader.join().unwrap();
        let selected = temp.path().join("selected");
        let entries: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(entries.len(), 1);
        if matches!(phase, "copy" | "synced") {
            assert!(!selected.exists());
            assert!(
                entries[0]
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".emilybase-create-")
            );
            let length = if phase == "copy" { 8192 } else { 20000 };
            assert_eq!(std::fs::read(&entries[0]).unwrap(), vec![0x59; length]);
        } else {
            assert_eq!(entries[0], selected);
            let length = if phase == "empty" { 0 } else { 20000 };
            assert_eq!(std::fs::read(selected).unwrap(), vec![0x59; length]);
        }
    }
}
