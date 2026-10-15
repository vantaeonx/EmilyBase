use super::*;
use crate::TEST_IO;
use crate::native_root::publication_tests::{changed, copy_directory, fixture};
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const FRESH: FileId = FileId::from_bytes([6; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([7; 16]);
fn object(path: &Path) -> PathBuf {
    path.join("objects/03030303030303030303030303030303.object")
}
fn change(path: &Path, saved: &Path, kind: usize) {
    match kind {
        0..=6 => changed(path, saved, kind),
        7 => {
            let path = object(path);
            fs::rename(&path, saved).unwrap();
            fs::copy(saved, path).unwrap();
        }
        _ => {
            let path = object(path);
            let bytes = fs::read(&path).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }
}

#[test]
fn native_reader_keeps_original_object_and_both_owners_with_exact_cursor_and_metadata() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let fds = fs::read_dir("/proc/self/fd").unwrap().count();
        let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let before = root.inspect().unwrap();
        let expected = root.file_info(FILE).unwrap().unwrap();
        let mut reader = root.reader(FILE).unwrap();
        assert_eq!(reader.info(), &expected);
        assert_eq!(reader.payload_position(), 0);
        assert!(std::mem::size_of::<NativeFileReader<'_>>() < 2048);
        let inode = fs::metadata(object(&path)).unwrap();
        assert!(
            fs::read_dir("/proc/self/fd")
                .unwrap()
                .filter_map(|e| e.ok())
                .filter_map(|e| fs::metadata(e.path()).ok())
                .any(|m| m.dev() == inode.dev() && m.ino() == inode.ino())
        );
        assert!(Database::open(path.join("metadata")).is_err());
        assert!(matches!(
            ProjectDirectory::open(path.join("objects"), PROJECT),
            Err(emilybase_object_storage::Error::Busy)
        ));
        assert!(!format!("{reader:?}").contains("existing"));
        let mut bytes = [0x77; 16];
        let read = reader.read_payload(&mut bytes).unwrap();
        assert_eq!(read, 3);
        assert_eq!(&bytes[..3], b"old");
        assert_eq!(&bytes[3..], &[0x77; 13]);
        assert_eq!(reader.read_payload(&mut bytes).unwrap(), 0);
        assert_eq!(reader.seek_payload(SeekFrom::End(-2)).unwrap(), 1);
        reader.verify().unwrap();
        assert_eq!(reader.payload_position(), 1);
        assert_eq!(reader.read_payload(&mut bytes).unwrap(), 2);
        assert_eq!(&bytes[..2], b"ld");
        assert_eq!(reader.finish().unwrap(), expected);
        assert_eq!(root.inspect().unwrap(), before);
        assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
        assert!(matches!(
            root.reader(FileId::from_bytes([5; 16])),
            Err(Error::Missing)
        ));
        assert_eq!(root.inspect().unwrap(), before);
        drop(root);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), fds);
    }
}

#[test]
fn bounded_empty_maximum_readonly_and_invalid_seek_preserve_original_bytes_and_state() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for length in [
            0,
            1,
            8192,
            8193,
            emilybase_object_storage::MAX_PAYLOAD_BYTES,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = fixture(temp.path(), compacted);
            let payload = (0..length).map(|i| (i % 251) as u8).collect::<Vec<_>>();
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let quota = root.quota_state().unwrap();
            root.set_quota(quota, FileQuota::new(8, length as u64 + 9).unwrap())
                .unwrap();
            let info = root
                .publish(FRESH, OBJECT, [8; 16], "private-display", &payload)
                .unwrap();
            fs::set_permissions(
                path.join(format!("objects/{OBJECT}.object")),
                fs::Permissions::from_mode(0o400),
            )
            .unwrap();
            let mut reader = root.reader(FRESH).unwrap();
            for seek in [SeekFrom::Current(-1), SeekFrom::End(-(length as i64) - 1)] {
                assert!(matches!(
                    reader.seek_payload(seek),
                    Err(Error::Objects(emilybase_object_storage::Error::InvalidSeek))
                ));
                assert_eq!(reader.payload_position(), 0);
            }
            assert_eq!(
                reader.seek_payload(SeekFrom::Start(u64::MAX)).unwrap(),
                u64::MAX
            );
            assert!(reader.seek_payload(SeekFrom::Current(1)).is_err());
            assert_eq!(reader.payload_position(), u64::MAX);
            let mut buffer = vec![0x77; 3 * MAX_OBJECT_READ_BYTES];
            assert_eq!(reader.read_payload(&mut buffer).unwrap(), 0);
            assert!(buffer.iter().all(|b| *b == 0x77));
            reader.seek_payload(SeekFrom::Start(0)).unwrap();
            let mut offset = 0;
            loop {
                buffer.fill(0x77);
                let n = reader.read_payload(&mut buffer).unwrap();
                assert!(n <= MAX_OBJECT_READ_BYTES);
                assert_eq!(&buffer[..n], &payload[offset..offset + n]);
                assert!(buffer[n..].iter().all(|b| *b == 0x77));
                offset += n;
                if n == 0 {
                    break;
                }
            }
            assert_eq!(offset, length);
            assert_eq!(reader.finish().unwrap(), info);
        }
    }
}

#[test]
fn admission_copy_seek_and_verification_reject_same_byte_source_substitutions() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in 0..4 {
            for kind in 0..9 {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let saved = temp.path().join("saved");
                let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                if phase == 0 {
                    let result = root.reader_with(FILE, || change(&path, &saved, kind));
                    assert!(result.is_err(), "phase={phase} kind={kind}");
                    drop(result);
                } else {
                    let mut reader = root.reader(FILE).unwrap();
                    let mut buffer = [0x77; 32];
                    let result = match phase {
                        1 => reader
                            .read_payload_with(&mut buffer, || change(&path, &saved, kind))
                            .map(|_| ()),
                        2 => reader
                            .seek_payload_with(SeekFrom::Start(1), || change(&path, &saved, kind))
                            .map(|_| ()),
                        _ => reader.verify_with(|| change(&path, &saved, kind)),
                    };
                    assert!(result.is_err(), "phase={phase} kind={kind}");
                    assert_eq!(reader.payload_position(), 0);
                    if phase == 1 {
                        assert_eq!(&buffer[..3], &[0; 3]);
                        assert_eq!(&buffer[3..], &[0x77; 29]);
                    }
                    assert!(matches!(
                        reader.read_payload(&mut buffer),
                        Err(Error::Poisoned)
                    ));
                    assert!(matches!(
                        reader.seek_payload(SeekFrom::Start(0)),
                        Err(Error::Poisoned)
                    ));
                    assert!(matches!(reader.finish(), Err(Error::Poisoned)));
                }
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
            }
        }
    }
}

#[test]
fn parent_replacement_cannot_redirect_stream_and_final_graph_rejects_changed_orphan() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in 0..4 {
            let temp = tempfile::tempdir().unwrap();
            let parent = temp.path().join("parent");
            fs::create_dir(&parent).unwrap();
            let path = fixture(&parent, compacted);
            let moved = temp.path().join("moved");
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let change = || {
                fs::rename(&parent, &moved).unwrap();
                fs::create_dir(&parent).unwrap();
                copy_directory(&moved.join("root"), &path);
            };
            if phase == 0 {
                assert!(root.reader_with(FILE, change).is_err());
            } else {
                let mut reader = root.reader(FILE).unwrap();
                let result = match phase {
                    1 => reader
                        .read_payload_with(&mut [0x77; 16], change)
                        .map(|_| ()),
                    2 => reader
                        .seek_payload_with(SeekFrom::Start(1), change)
                        .map(|_| ()),
                    _ => reader.verify_with(change),
                };
                assert!(result.is_err());
                assert_eq!(reader.payload_position(), 0);
            }
            assert!(matches!(root.inspect(), Err(Error::Poisoned)));
        }
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), compacted);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let mut reader = root.reader(FILE).unwrap();
        let mut buffer = [0; 8];
        assert_eq!(reader.read_payload(&mut buffer).unwrap(), 3);
        assert_eq!(&buffer[..3], b"old");
        let orphan = path.join("objects/05050505050505050505050505050505.object");
        let mut bytes = fs::read(&orphan).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&orphan, &bytes).unwrap();
        assert!(reader.finish().is_err());
        assert!(matches!(root.inspect(), Err(Error::Poisoned)));
        assert_eq!(fs::read(orphan).unwrap(), bytes);
    }
}

#[test]
fn eof_empty_read_and_replaced_source_before_read_clear_only_the_attempted_prefix() {
    let _serial = TEST_IO.lock().unwrap();
    for mode in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let path = fixture(temp.path(), false);
        let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
        let mut reader = root.reader(FILE).unwrap();
        if mode == 0 {
            reader.seek_payload(SeekFrom::End(0)).unwrap();
        }
        changed(&path, &temp.path().join("saved"), 0);
        let mut bytes = [0x77; 32];
        let result = if mode == 1 {
            reader.read_payload(&mut [])
        } else {
            reader.read_payload(&mut bytes)
        };
        assert!(result.is_err());
        let cleared = if mode == 2 { 3 } else { 0 };
        assert!(bytes[..cleared].iter().all(|b| *b == 0));
        assert!(bytes[cleared..].iter().all(|b| *b == 0x77));
        assert_eq!(reader.payload_position(), if mode == 0 { 3 } else { 0 });
    }
}

#[test]
#[ignore = "native retained stream helper invoked by parent kill test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_NATIVE_STREAM_TEST_PARENT") else {
        return;
    };
    let phase = std::env::var("EMILYBASE_NATIVE_STREAM_TEST_PHASE").unwrap();
    let ready = || -> ! {
        println!("EB_NATIVE_FILE_ROOT_STREAM_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let mut root = NativeFileRoot::open(PathBuf::from(parent).join("root"), PROJECT).unwrap();
    let mut reader = root
        .reader_with(FRESH, || {
            if phase == "admitted" {
                ready();
            }
        })
        .unwrap();
    loop {
        let n = reader
            .read_payload_with(&mut [0; 8192], || {
                if phase == "copied" {
                    ready();
                }
            })
            .unwrap();
        if n == 0 {
            break;
        }
    }
    reader
        .verify_with(|| {
            if phase == "verified" {
                ready();
            }
        })
        .unwrap();
    reader.finish().unwrap();
    assert_eq!(phase, "finished");
    ready();
}

#[test]
fn killed_streams_release_owners_and_preserve_exact_history_payload_and_next_write() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["admitted", "copied", "verified", "finished"] {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let payload = if empty { vec![] } else { vec![0x53; 8193] };
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                root.publish(FRESH, OBJECT, [8; 16], "synthetic", &payload)
                    .unwrap();
                let expected = root.inspect().unwrap();
                drop(root);
                let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "native_root::reader::tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_NATIVE_STREAM_TEST_PARENT", temp.path())
                    .env("EMILYBASE_NATIVE_STREAM_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_NATIVE_FILE_ROOT_STREAM_READY" {
                            send.send(()).unwrap();
                            return;
                        }
                    }
                });
                let ready = receive.recv_timeout(std::time::Duration::from_secs(15));
                child.kill().unwrap();
                let status = child.wait().unwrap();
                thread.join().unwrap();
                assert!(ready.is_ok());
                assert_eq!(status.signal(), Some(9));
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                assert_eq!(root.inspect().unwrap(), expected);
                assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
                let mut reader = root.reader(FRESH).unwrap();
                let mut actual = Vec::new();
                let mut scratch = [0; 8192];
                loop {
                    let n = reader.read_payload(&mut scratch).unwrap();
                    actual.extend_from_slice(&scratch[..n]);
                    if n == 0 {
                        break;
                    }
                }
                reader.finish().unwrap();
                assert_eq!(actual, payload);
                root.rename(FRESH, 4, "next").unwrap();
                assert_eq!(root.inspect().unwrap().revision(), 5);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_slices_and_seeks_match_independent_payload_cursor(compacted in any::<bool>(), payload in prop::collection::vec(any::<u8>(),0..8194), steps in prop::collection::vec((0u8..3,0usize..16385),1..33)) {
        let _serial=TEST_IO.lock().unwrap();let temp=tempfile::tempdir().unwrap();let path=fixture(temp.path(),compacted);let mut root=NativeFileRoot::open(&path,PROJECT).unwrap();root.publish(FRESH,OBJECT,[8;16],"synthetic",&payload).unwrap();let before=root.inspect().unwrap();let mut reader=root.reader(FRESH).unwrap();let mut position=0;
        for (kind,size) in steps {if kind==0 {reader.seek_payload(SeekFrom::Start(size as u64)).unwrap();position=size;} else if kind==1 {reader.verify().unwrap();} else {let mut bytes=vec![0x77;size];let n=reader.read_payload(&mut bytes).unwrap();let expected=size.min(MAX_OBJECT_READ_BYTES).min(payload.len().saturating_sub(position));prop_assert_eq!(n,expected);if n>0 {prop_assert_eq!(&bytes[..n],&payload[position..position+n]);}prop_assert!(bytes[n..].iter().all(|b|*b==0x77));position+=n;}prop_assert_eq!(reader.payload_position(),position as u64);}
        reader.finish().unwrap();prop_assert_eq!(root.inspect().unwrap(),before.clone());drop(root);prop_assert_eq!(NativeFileRoot::open(&path,PROJECT).unwrap().inspect().unwrap(),before);
    }
}
