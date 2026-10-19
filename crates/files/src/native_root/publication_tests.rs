use super::*;
use crate::{FileArchiveReader, TEST_IO, initialize_file_root, restore_file_archive};
use proptest::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::{Command, Stdio};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FRESH: FileId = FileId::from_bytes([6; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([7; 16]);
mod guarded;
pub(super) fn fixture(parent: &Path, compacted: bool) -> PathBuf {
    let path = parent.join("root");
    initialize_file_root(&path, PROJECT, FileQuota::new(8, 32768).unwrap()).unwrap();
    let mut database = Database::open(path.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    let objects = ProjectDirectory::open(path.join("objects"), PROJECT).unwrap();
    let mut store = FileStore::open(database, objects).unwrap();
    store
        .publish(
            FileId::from_bytes([2; 16]),
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "existing",
            b"old",
        )
        .unwrap();
    store
        .objects
        .put(ObjectId::from_bytes([5; 16]), b"orphan")
        .unwrap();
    path
}
pub(super) fn components(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap()
}
pub(super) fn copy_directory(source: &Path, target: &Path) {
    fs::DirBuilder::new().mode(0o700).create(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}
pub(super) fn changed(path: &Path, saved: &Path, kind: usize) {
    match kind {
        0 => {
            fs::rename(path, saved).unwrap();
            copy_directory(saved, path);
        }
        1 | 2 => {
            let child = path.join(if kind == 1 { "metadata" } else { "objects" });
            fs::rename(&child, saved).unwrap();
            copy_directory(saved, &child);
        }
        3 | 4 => {
            let file = path.join(if kind == 3 {
                "metadata/redo.wal"
            } else {
                "objects/.emilybase-objects"
            });
            fs::rename(&file, saved).unwrap();
            fs::copy(saved, file).unwrap();
        }
        5 => fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap(),
        _ => {
            fs::write(path.join("unexpected"), b"protected").unwrap();
        }
    }
}

#[test]
fn guarded_publication_advances_only_original_wal_and_keeps_root_reusable_on_both_versions() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = fixture(temp.path(), compacted);
            let payload = if empty { vec![] } else { vec![0x53; 8193] };
            let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let before = root.inspect().unwrap();
            let result = root
                .publish(FRESH, OBJECT, [8; 16], "fresh-display", &payload)
                .unwrap();
            assert_eq!(result.revision(), 4);
            let after = root.inspect().unwrap();
            assert_eq!(after.revision(), 4);
            assert_eq!(after.database_id(), before.database_id());
            assert_eq!(after.quota(), before.quota());
            assert_eq!(
                after.usage(),
                FileUsage {
                    physical_objects: 3,
                    payload_bytes: 9 + payload.len() as u64,
                    references: 2,
                    orphans: 1
                }
            );
            let snapshot = root.capture().unwrap();
            assert_eq!(
                snapshot.metadata_report().wal_version,
                if compacted { 2 } else { 1 }
            );
            assert!(snapshot.files().contains(&result));
            assert_eq!(
                snapshot
                    .objects()
                    .objects()
                    .iter()
                    .find(|o| o.object() == OBJECT)
                    .unwrap()
                    .payload(),
                payload
            );
            assert!(matches!(
                root.publish(
                    FRESH,
                    ObjectId::from_bytes([9; 16]),
                    [8; 16],
                    "duplicate",
                    b"refused"
                ),
                Err(Error::Exists)
            ));
            assert_eq!(root.inspect().unwrap(), after);
            drop(root);
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
            assert_eq!(
                NativeFileRoot::open(&path, PROJECT)
                    .unwrap()
                    .inspect()
                    .unwrap(),
                after
            );
        }
    }
}

#[test]
fn invalid_name_and_capacity_refusal_preserve_wal_inventory_and_owner_usability() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = fixture(temp.path(), false);
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    let before = root.inspect().unwrap();
    let wal = fs::read(path.join("metadata/redo.wal")).unwrap();
    for name in ["", "\n", &"x".repeat(crate::MAX_FILE_NAME_BYTES + 1)] {
        assert!(matches!(
            root.publish(FRESH, OBJECT, [8; 16], name, b"payload"),
            Err(Error::Name)
        ));
        assert_eq!(root.inspect().unwrap(), before);
    }
    assert!(
        root.publish(
            FRESH,
            OBJECT,
            [8; 16],
            "too-large",
            &vec![0; emilybase_object_storage::MAX_PAYLOAD_BYTES + 1]
        )
        .is_err()
    );
    assert_eq!(root.inspect().unwrap(), before);
    assert_eq!(fs::read(path.join("metadata/redo.wal")).unwrap(), wal);
    drop(root);
    let mut store = components(&path);
    let state = store.quota_state().unwrap();
    store
        .set_quota(state, FileQuota::new(2, 9).unwrap())
        .unwrap();
    drop(store);
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    let before = root.inspect().unwrap();
    assert!(
        root.publish(FRESH, OBJECT, [8; 16], "no-capacity", &[])
            .is_err()
    );
    assert_eq!(root.inspect().unwrap(), before);
}

#[test]
fn detected_source_changes_before_commit_leave_orphan_and_after_commit_keep_acknowledged_reference()
{
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [
            PublishBoundary::Blob,
            PublishBoundary::Metadata,
            PublishBoundary::Verified,
        ] {
            for kind in 0..7 {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let result = root.publish_with(FRESH, OBJECT, [8; 16], "fresh", b"payload", |at| {
                    if at == phase {
                        changed(&path, &temp.path().join("saved"), kind);
                    }
                });
                assert!(
                    matches!(result, Err(Error::OutcomeUnknown(_))),
                    "kind={kind}"
                );
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                let mut store = components(&path);
                let committed = phase != PublishBoundary::Blob;
                assert_eq!(
                    store.database.last_transaction(),
                    if committed { 4 } else { 3 }
                );
                assert_eq!(store.info(FRESH).unwrap().is_some(), committed);
                let usage = store.usage().unwrap();
                assert_eq!(usage.physical_objects, 3);
                assert_eq!(usage.references, if committed { 2 } else { 1 });
                assert_eq!(usage.orphans, if committed { 1 } else { 2 });
                let snapshot = store.capture().unwrap();
                assert_eq!(
                    snapshot
                        .objects()
                        .objects()
                        .iter()
                        .find(|o| o.object() == OBJECT)
                        .unwrap()
                        .payload(),
                    b"payload"
                );
            }
        }
    }
}

#[test]
fn changed_source_before_any_publication_is_direct_and_creates_no_new_blob() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = fixture(temp.path(), false);
    let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
    changed(&path, &temp.path().join("saved"), 0);
    let result = root.publish(FRESH, OBJECT, [8; 16], "fresh", b"payload");
    assert!(result.is_err());
    assert!(!matches!(result, Err(Error::OutcomeUnknown(_))));
    drop(root);
    for path in [&path, &temp.path().join("saved")] {
        let store = components(path);
        assert_eq!(store.usage().unwrap().physical_objects, 2);
        assert!(store.info(FRESH).unwrap().is_none());
    }
}

#[test]
fn parent_replacement_never_redirects_guarded_commit_or_acknowledges_the_replacement() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [
            PublishBoundary::Blob,
            PublishBoundary::Metadata,
            PublishBoundary::Verified,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let parent = temp.path().join("parent");
            let moved = temp.path().join("moved");
            fs::create_dir(&parent).unwrap();
            let path = fixture(&parent, compacted);
            let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
            let result = root.publish_with(FRESH, OBJECT, [8; 16], "fresh", b"payload", |at| {
                if at == phase {
                    fs::rename(&parent, &moved).unwrap();
                    fs::create_dir(&parent).unwrap();
                    copy_directory(&moved.join("root"), &path);
                }
            });
            assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
            assert!(matches!(root.inspect(), Err(Error::Poisoned)));
            drop(root);
            let expected = if phase == PublishBoundary::Blob { 3 } else { 4 };
            for path in [&path, &moved.join("root")] {
                let store = components(path);
                assert_eq!(store.database.last_transaction(), expected);
                assert_eq!(store.info(FRESH).unwrap().is_some(), expected == 4);
                assert_eq!(store.usage().unwrap().physical_objects, 3);
            }
        }
    }
}

#[test]
fn same_inode_wal_rewrite_or_bad_mode_is_never_adopted_as_the_owners_commit_readback() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for phase in [PublishBoundary::Blob, PublishBoundary::Metadata] {
            for kind in 0..4 {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let mut root = NativeFileRoot::open(&path, PROJECT).unwrap();
                let wal = path.join("metadata/redo.wal");
                let mut changed = Vec::new();
                let result = root.publish_with(FRESH, OBJECT, [8; 16], "fresh", b"payload", |at| {
                    if at != phase {
                        return;
                    }
                    changed = fs::read(&wal).unwrap();
                    match kind {
                        0 => fs::write(&wal, &changed).unwrap(),
                        1 => {
                            *changed.last_mut().unwrap() ^= 1;
                            fs::write(&wal, &changed).unwrap();
                        }
                        2 => {
                            fs::set_permissions(&wal, fs::Permissions::from_mode(0o644)).unwrap();
                        }
                        _ => {
                            changed.extend_from_slice(b"foreign tail");
                            fs::write(&wal, &changed).unwrap();
                        }
                    }
                });
                assert!(
                    matches!(result, Err(Error::OutcomeUnknown(_))),
                    "kind={kind}"
                );
                assert!(matches!(root.inspect(), Err(Error::Poisoned)));
                drop(root);
                assert_eq!(fs::read(&wal).unwrap(), changed);
            }
        }
    }
}

#[test]
#[ignore = "native guarded publication helper invoked by parent kill test"]
fn worker() {
    let Some(parent) = std::env::var_os("EMILYBASE_NATIVE_PUBLISH_TEST_PARENT") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let phase = std::env::var("EMILYBASE_NATIVE_PUBLISH_TEST_PHASE").unwrap();
    let payload = fs::read(parent.join("payload")).unwrap();
    let ready = || -> ! {
        println!("EB_NATIVE_FILE_ROOT_PUBLISH_READY");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    let mut root = NativeFileRoot::open(parent.join("root"), PROJECT).unwrap();
    if phase == "guard" {
        let at = std::env::var("EMILYBASE_NATIVE_GUARD_PHASE").unwrap();
        let kind = std::env::var("EMILYBASE_NATIVE_GUARD_KIND")
            .unwrap()
            .parse()
            .unwrap();
        guarded::run_with_payload(&mut root, kind, &payload, |phase| {
            if guarded::label(phase) == at {
                ready();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(at, "returned");
        ready();
    }
    root.publish_with(FRESH, OBJECT, [8; 16], "fresh", &payload, |at| {
        if matches!(
            (phase.as_str(), at),
            ("blob", PublishBoundary::Blob)
                | ("metadata", PublishBoundary::Metadata)
                | ("verified", PublishBoundary::Verified)
        ) {
            ready();
        }
    })
    .unwrap();
    assert_eq!(phase, "ack");
    ready();
}

#[test]
fn killed_root_publishers_recover_exact_reference_visibility_and_physical_charge() {
    use std::os::unix::process::ExitStatusExt;
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for empty in [false, true] {
            for phase in ["blob", "metadata", "verified", "ack"] {
                let temp = tempfile::tempdir().unwrap();
                let path = fixture(temp.path(), compacted);
                let payload = if empty { vec![] } else { vec![0x53; 8193] };
                fs::write(temp.path().join("payload"), &payload).unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "native_root::publication_tests::worker",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("EMILYBASE_NATIVE_PUBLISH_TEST_PARENT", temp.path())
                    .env("EMILYBASE_NATIVE_PUBLISH_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let stdout = child.stdout.take().unwrap();
                let (send, receive) = std::sync::mpsc::channel();
                let thread = std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        if line.unwrap() == "EB_NATIVE_FILE_ROOT_PUBLISH_READY" {
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
                let info = root.inspect().unwrap();
                let committed = phase != "blob";
                assert_eq!(info.revision(), if committed { 4 } else { 3 });
                assert_eq!(info.usage().references, if committed { 2 } else { 1 });
                assert_eq!(info.usage().orphans, if committed { 1 } else { 2 });
                assert_eq!(info.usage().physical_objects, 3);
                let snapshot = root.capture().unwrap();
                assert_eq!(
                    snapshot
                        .objects()
                        .objects()
                        .iter()
                        .find(|o| o.object() == OBJECT)
                        .unwrap()
                        .payload(),
                    payload
                );
                root.publish(
                    FileId::from_bytes([10; 16]),
                    ObjectId::from_bytes([11; 16]),
                    [8; 16],
                    "next",
                    b"independent",
                )
                .unwrap();
                assert_eq!(
                    root.inspect().unwrap().revision(),
                    if committed { 5 } else { 4 }
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_guarded_writes_reopen_and_restore_against_independent_visibility_charge_model(compacted in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..513),keys in prop::collection::vec(0u8..8,1..13)) {
        let _serial=TEST_IO.lock().unwrap();let temp=tempfile::tempdir().unwrap();let path=fixture(temp.path(),compacted);let mut root=NativeFileRoot::open(&path,PROJECT).unwrap();let mut model=std::collections::BTreeSet::new();
        for (step,key) in keys.into_iter().enumerate() {
            let accepted=!model.contains(&key) && model.len()<6;
            let result=root.publish(FileId::from_bytes([6+key;16]),ObjectId::from_bytes([30+step as u8;16]),[8;16],"synthetic",&payload);prop_assert_eq!(result.is_ok(),accepted);if accepted {model.insert(key);}
            let info=root.inspect().unwrap();prop_assert_eq!(info.revision(),3+model.len() as u64);prop_assert_eq!(info.usage().references,1+model.len());prop_assert_eq!(info.usage().physical_objects,2+model.len());prop_assert_eq!(info.usage().orphans,1);prop_assert_eq!(info.usage().payload_bytes,9+model.len() as u64*payload.len() as u64);
            drop(root);root=NativeFileRoot::open(&path,PROJECT).unwrap();prop_assert_eq!(root.inspect().unwrap(),info);
        }
        let expected=root.inspect().unwrap();let snapshot=root.capture().unwrap();let mut bytes=Vec::new();FileArchiveReader::from_snapshot(&snapshot).unwrap().read_to_end(&mut bytes).unwrap();restore_file_archive(&bytes,PROJECT,temp.path().join("copy")).unwrap();prop_assert_eq!(NativeFileRoot::open(temp.path().join("copy"),PROJECT).unwrap().inspect().unwrap(),expected);
    }
}
