//! Test-only sync/kill boundaries; release builds contain no environment hooks.
use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{
    BPlusTree, IndexSnapshot, IndexStore, Key, MAX_SNAPSHOT_BYTES, RecordPointer, StoreError,
};

// Serialize filesystem unit tests while fork briefly inherits unrelated owners before exec.
static PROCESS_TESTS: Mutex<()> = Mutex::new(());
thread_local! {
    static FAULT: RefCell<Option<(&'static str,bool)>> = const {RefCell::new(None)};
}
pub(crate) fn fail(boundary: &str, after: bool) -> std::io::Result<()> {
    FAULT.with_borrow_mut(|fault| {
        if fault
            .as_ref()
            .is_some_and(|selected| selected.0 == boundary && selected.1 == after)
        {
            *fault = None;
            Err(std::io::Error::other("injected index sync failure"))
        } else {
            Ok(())
        }
    })
}
fn inject(boundary: &'static str, after: bool) {
    FAULT.with_borrow_mut(|fault| *fault = Some((boundary, after)));
}
pub(crate) fn checkpoint(point: &str) {
    if std::env::var("EMILYBASE_INDEX_KILL_POINT").as_deref() == Ok(point) {
        println!("INDEX_BOUNDARY {point}");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
fn pointer(key: i64) -> RecordPointer {
    RecordPointer {
        page_id: key as u64 + 1,
        slot_id: key as u16,
    }
}
fn original() -> BPlusTree {
    BPlusTree::from_sorted_stable(
        &(0..225)
            .map(|key| (Key::Integer(key), pointer(key)))
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
fn changed() -> BPlusTree {
    let mut tree = original();
    for key in 20..50 {
        tree.remove(&Key::Integer(key)).unwrap();
    }
    for key in 500..540 {
        tree.insert(Key::Integer(key), pointer(key)).unwrap();
    }
    tree.replace(&Key::Integer(100), pointer(900)).unwrap();
    tree
}
fn active(root: &Path) -> std::path::PathBuf {
    root.join("tree.ebif")
}

#[test]
fn store_reopens_exact_revisions_and_holds_ownership_across_inode_replacement() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    let mut store = IndexStore::create(&root, &original()).unwrap();
    assert_eq!(
        store.snapshot().unwrap(),
        &IndexSnapshot {
            revision: 1,
            tree: original()
        }
    );
    assert!(matches!(IndexStore::open(&root), Err(StoreError::Busy)));
    let next = changed();
    assert_eq!(store.replace(&next).unwrap(), 2);
    assert!(matches!(IndexStore::open(&root), Err(StoreError::Busy)));
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(active(&root)).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(store);
    let mut store = IndexStore::open(&root).unwrap();
    assert_eq!(
        store.snapshot().unwrap(),
        &IndexSnapshot {
            revision: 2,
            tree: next.clone()
        }
    );
    let delta = store.snapshot().unwrap().delta_to(&original()).unwrap();
    assert_eq!(store.apply(&delta).unwrap(), 3);
    let bytes = fs::read(active(&root)).unwrap();
    assert!(store.apply(&delta).is_err());
    assert_eq!(fs::read(active(&root)).unwrap(), bytes);
    assert_eq!(store.replace(&next).unwrap(), 4);
}

#[test]
fn create_refuses_existing_files_directories_symlinks_and_invalid_trees() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("existing-file");
    fs::write(&file, b"synthetic-preserved").unwrap();
    let dir = temp.path().join("existing-dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("keep"), b"synthetic-preserved").unwrap();
    let link = temp.path().join("existing-link");
    symlink(&dir, &link).unwrap();
    for path in [&file, &dir, &link] {
        assert!(IndexStore::create(path, &original()).is_err());
    }
    assert_eq!(fs::read(&file).unwrap(), b"synthetic-preserved");
    assert_eq!(fs::read(dir.join("keep")).unwrap(), b"synthetic-preserved");
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    let invalid = temp.path().join("invalid");
    assert!(IndexStore::create(&invalid, &BPlusTree::new()).is_err());
    assert!(!invalid.exists());
}

#[test]
fn store_rejects_unsafe_root_or_active_paths_and_never_adopts_staging() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    drop(IndexStore::create(&root, &original()).unwrap());
    let bytes = fs::read(active(&root)).unwrap();
    let staging = root.join(".index-stage-leftover");
    fs::write(
        &staging,
        IndexSnapshot {
            revision: 90,
            tree: changed(),
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    drop(IndexStore::open(&root).unwrap());
    assert_eq!(fs::read(active(&root)).unwrap(), bytes);
    fs::remove_file(active(&root)).unwrap();
    assert!(IndexStore::open(&root).is_err());
    fs::write(active(&root), &bytes).unwrap();
    fs::set_permissions(active(&root), fs::Permissions::from_mode(0o600)).unwrap();
    let alias = temp.path().join("root-link");
    symlink(&root, &alias).unwrap();
    assert!(matches!(
        IndexStore::open(&alias),
        Err(StoreError::PrivatePath)
    ));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        IndexStore::open(&root),
        Err(StoreError::PrivatePath)
    ));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(active(&root), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        IndexStore::open(&root),
        Err(StoreError::PrivatePath)
    ));
    fs::set_permissions(active(&root), fs::Permissions::from_mode(0o600)).unwrap();
    let hard = temp.path().join("hard-link");
    fs::hard_link(active(&root), &hard).unwrap();
    assert!(matches!(
        IndexStore::open(&root),
        Err(StoreError::PrivatePath)
    ));
    fs::remove_file(hard).unwrap();
    fs::remove_file(active(&root)).unwrap();
    symlink(&staging, active(&root)).unwrap();
    assert!(matches!(
        IndexStore::open(&root),
        Err(StoreError::PrivatePath)
    ));
}

#[test]
fn damaged_or_oversized_active_snapshot_fails_closed_without_mutation() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    drop(IndexStore::create(&root, &BPlusTree::new_stable()).unwrap());
    let bytes = fs::read(active(&root)).unwrap();
    for cut in [0, 64, 4095, 4096, 8191] {
        fs::write(active(&root), &bytes[..cut]).unwrap();
        assert!(IndexStore::open(&root).is_err());
        assert_eq!(fs::read(active(&root)).unwrap(), bytes[..cut]);
    }
    let mut corrupt = bytes.clone();
    corrupt[4096 + 100] ^= 1;
    fs::write(active(&root), &corrupt).unwrap();
    assert!(IndexStore::open(&root).is_err());
    assert_eq!(fs::read(active(&root)).unwrap(), corrupt);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(active(&root))
        .unwrap();
    file.set_len(MAX_SNAPSHOT_BYTES as u64 + 1).unwrap();
    assert!(IndexStore::open(&root).is_err());
    assert_eq!(
        file.metadata().unwrap().len(),
        MAX_SNAPSHOT_BYTES as u64 + 1
    );
}

#[test]
fn external_valid_snapshot_change_poisons_owner_until_reopen() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    let mut store = IndexStore::create(&root, &original()).unwrap();
    let changed_bytes = IndexSnapshot {
        revision: 1,
        tree: changed(),
    }
    .encode()
    .unwrap();
    fs::write(active(&root), &changed_bytes).unwrap();
    assert!(matches!(
        store.replace(&original()),
        Err(StoreError::Changed)
    ));
    assert!(matches!(store.snapshot(), Err(StoreError::Poisoned)));
    assert!(matches!(
        store.replace(&original()),
        Err(StoreError::Poisoned)
    ));
    assert_eq!(fs::read(active(&root)).unwrap(), changed_bytes);
    assert!(matches!(IndexStore::open(&root), Err(StoreError::Busy)));
    drop(store);
    assert_eq!(
        IndexStore::open(&root).unwrap().snapshot().unwrap().tree,
        changed()
    );
}

#[test]
fn changed_root_inode_cannot_redirect_a_locked_owner_into_another_directory() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    let moved = temp.path().join("moved");
    let mut store = IndexStore::create(&root, &original()).unwrap();
    fs::rename(&root, &moved).unwrap();
    drop(IndexStore::create(&root, &original()).unwrap());
    let original_bytes = fs::read(active(&root)).unwrap();
    assert!(matches!(
        store.replace(&changed()),
        Err(StoreError::Changed)
    ));
    assert!(matches!(store.snapshot(), Err(StoreError::Poisoned)));
    assert_eq!(fs::read(active(&root)).unwrap(), original_bytes);
    assert_eq!(fs::read(active(&moved)).unwrap(), original_bytes);
}

#[test]
fn changed_root_permissions_prevent_publication_and_poison_the_owner() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    let mut store = IndexStore::create(&root, &original()).unwrap();
    let before = fs::read(active(&root)).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        store.replace(&changed()),
        Err(StoreError::PrivatePath)
    ));
    assert!(matches!(store.snapshot(), Err(StoreError::Poisoned)));
    assert_eq!(fs::read(active(&root)).unwrap(), before);
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    drop(store);
    assert_eq!(
        IndexStore::open(&root)
            .unwrap()
            .snapshot()
            .unwrap()
            .revision,
        1
    );
}

#[test]
fn prepublication_sync_errors_preserve_old_state_and_allow_retry() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for after in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("index");
        let mut store = IndexStore::create(&root, &original()).unwrap();
        let before = fs::read(active(&root)).unwrap();
        inject("replace_file_sync", after);
        assert!(matches!(store.replace(&changed()), Err(StoreError::Io(_))));
        assert_eq!(store.snapshot().unwrap().revision, 1);
        assert_eq!(fs::read(active(&root)).unwrap(), before);
        assert_eq!(store.replace(&changed()).unwrap(), 2);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }
}

#[test]
fn postrename_sync_errors_report_unknown_keep_ownership_and_reopen_complete_revision() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for after in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("index");
        let mut store = IndexStore::create(&root, &original()).unwrap();
        inject("replace_directory_sync", after);
        assert!(matches!(
            store.replace(&changed()),
            Err(StoreError::PublicationUnknown(_))
        ));
        assert!(matches!(store.snapshot(), Err(StoreError::Poisoned)));
        assert!(matches!(IndexStore::open(&root), Err(StoreError::Busy)));
        drop(store);
        let mut reopened = IndexStore::open(&root).unwrap();
        assert_eq!(
            reopened.snapshot().unwrap(),
            &IndexSnapshot {
                revision: 2,
                tree: changed()
            }
        );
        assert_eq!(reopened.replace(&original()).unwrap(), 3);
    }
}

#[test]
fn create_sync_errors_preserve_absent_or_complete_publication_boundaries() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for point in [
        "create_file_sync",
        "create_directory_sync",
        "create_parent_sync",
    ] {
        for after in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("index");
            inject(point, after);
            let result = IndexStore::create(&root, &original());
            if point == "create_parent_sync" {
                assert!(matches!(result, Err(StoreError::PublicationUnknown(_))));
                assert_eq!(
                    IndexStore::open(&root).unwrap().snapshot().unwrap(),
                    &IndexSnapshot {
                        revision: 1,
                        tree: original()
                    }
                );
            } else {
                assert!(matches!(result, Err(StoreError::Io(_))));
                assert!(!root.exists());
                assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
            }
        }
    }
}

struct ChildGuard(std::process::Child);
impl std::ops::Deref for ChildGuard {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
fn child(root: &Path, action: &str, point: &str) -> ChildGuard {
    ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "store_tests::publication_helper",
                "--nocapture",
            ])
            .env("EMILYBASE_INDEX_TEST_ROOT", root)
            .env("EMILYBASE_INDEX_TEST_ACTION", action)
            .env("EMILYBASE_INDEX_KILL_POINT", point)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}
fn kill_at(root: &Path, action: &str, point: &str) {
    let mut process = child(root, action, point);
    let (send, receive) = std::sync::mpsc::channel();
    let stdout = process.stdout.take().unwrap();
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
            Ok(Ok(line)) if line == format!("INDEX_BOUNDARY {point}") => break true,
            Ok(Ok(_)) => (),
            _ => break false,
        }
    };
    process.kill().unwrap();
    assert!(!process.wait().unwrap().success());
    drop(receive);
    reader.join().unwrap();
    assert!(
        reached,
        "index publication boundary {point} was not reached"
    );
}

#[test]
fn process_kills_during_creation_only_publish_complete_snapshots() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for point in [
        "create_file_synced",
        "create_directory_synced",
        "create_renamed",
        "create_parent_synced",
        "create_ack",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("index");
        kill_at(&root, "create", point);
        if point == "create_file_synced" || point == "create_directory_synced" {
            assert!(!root.exists());
            assert!(IndexStore::open(&root).is_err());
        } else {
            let mut reopened = IndexStore::open(&root).unwrap();
            assert_eq!(
                reopened.snapshot().unwrap(),
                &IndexSnapshot {
                    revision: 1,
                    tree: original()
                }
            );
            assert_eq!(reopened.replace(&changed()).unwrap(), 2);
        }
    }
}

#[test]
fn replacement_kills_preserve_all_acknowledged_revisions_and_whole_trees() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    for point in [
        "replace_file_synced",
        "replace_renamed",
        "replace_directory_synced",
        "replace_ack",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("index");
        drop(IndexStore::create(&root, &original()).unwrap());
        kill_at(&root, "replace", point);
        let mut reopened = IndexStore::open(&root).unwrap();
        let expected = if point == "replace_file_synced" {
            IndexSnapshot {
                revision: 1,
                tree: original(),
            }
        } else {
            IndexSnapshot {
                revision: 2,
                tree: changed(),
            }
        };
        assert_eq!(reopened.snapshot().unwrap(), &expected);
        assert_eq!(
            reopened.replace(&original()).unwrap(),
            expected.revision + 1
        );
    }
}

#[test]
fn two_competing_processes_preserve_every_snapshot_increment() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("index");
    let mut tree = BPlusTree::new_stable();
    tree.insert(
        Key::Integer(0),
        RecordPointer {
            page_id: 1,
            slot_id: 0,
        },
    )
    .unwrap();
    drop(IndexStore::create(&root, &tree).unwrap());
    let mut first = child(&root, "counter", "");
    let mut second = child(&root, "counter", "");
    let deadline = Instant::now() + Duration::from_secs(15);
    for process in [&mut first, &mut second] {
        loop {
            if let Some(status) = process.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() > deadline {
                let _ = process.kill();
                let _ = process.wait();
                panic!("competing index writer deadline");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let store = IndexStore::open(&root).unwrap();
    assert_eq!(store.snapshot().unwrap().revision, 21);
    assert_eq!(
        store
            .snapshot()
            .unwrap()
            .tree
            .get(&Key::Integer(0))
            .unwrap(),
        Some(RecordPointer {
            page_id: 21,
            slot_id: 0
        })
    );
}

#[test]
#[ignore = "invoked in child processes by publication/concurrency tests"]
fn publication_helper() {
    let root = std::path::PathBuf::from(std::env::var_os("EMILYBASE_INDEX_TEST_ROOT").unwrap());
    match std::env::var("EMILYBASE_INDEX_TEST_ACTION")
        .unwrap()
        .as_str()
    {
        "create" => {
            let _store = IndexStore::create(&root, &original()).unwrap();
            checkpoint("create_ack");
        }
        "replace" => {
            let mut store = IndexStore::open(&root).unwrap();
            store.replace(&changed()).unwrap();
            checkpoint("replace_ack");
        }
        "counter" => {
            for _ in 0..10 {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut store = loop {
                    match IndexStore::open(&root) {
                        Ok(store) => break store,
                        Err(StoreError::Busy) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(1))
                        }
                        Err(error) => panic!("index writer failed: {error}"),
                    }
                };
                let mut tree = store.snapshot().unwrap().tree.clone();
                let mut value = tree.get(&Key::Integer(0)).unwrap().unwrap();
                value.page_id += 1;
                tree.replace(&Key::Integer(0), value).unwrap();
                store.replace(&tree).unwrap();
            }
        }
        _ => panic!("unknown private helper action"),
    }
}
