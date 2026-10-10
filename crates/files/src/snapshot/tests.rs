use super::*;
use crate::{FileId, FileUsage, TEST_IO};
use emilybase_catalog::{Key, Value};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
fn setup(path: &Path, compacted: bool) -> FileStore {
    let mut database = Database::create(path.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path.join("objects"))
        .unwrap();
    FileStore::initialize(
        database,
        ProjectDirectory::initialize(path.join("objects"), PROJECT).unwrap(),
        FileQuota::new(128, 64 * 1024 * 1024).unwrap(),
    )
    .unwrap()
}
fn open(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap()
}
// Two existing component restores exercise the captured graph, not common atomic
// publication. The caller keeps this synthetic parent private during the test.
fn component_round_trip(snapshot: &FileSnapshot, path: &Path) -> FileStore {
    emilybase_backup::restore_bytes(snapshot.metadata_bytes(), path.join("metadata")).unwrap();
    let bytes = emilybase_object_storage::encode_archive(snapshot.objects()).unwrap();
    emilybase_object_storage::restore_archive(&bytes, snapshot.project(), path.join("objects"))
        .unwrap();
    open(path)
}

#[test]
fn immutable_pair_preserves_metadata_orphans_quota_and_old_payload_after_source_changes() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut store = setup(temp.path(), compacted);
        let baseline = store.database.committed_wal().unwrap();
        let empty = store.capture().unwrap();
        assert!(empty.files().is_empty());
        assert!(empty.objects().objects().is_empty());
        assert_eq!(store.database.committed_wal().unwrap(), baseline);
        let payload = vec![0x53; 8193];
        let first = store
            .publish(FILE, OBJECT, [4; 16], "synthetic", &payload)
            .unwrap();
        let removed = FileId::from_bytes([5; 16]);
        let orphan = ObjectId::from_bytes([6; 16]);
        let info = store
            .publish(removed, orphan, [7; 16], "removed", b"orphan")
            .unwrap();
        store.remove(removed, info.revision()).unwrap();
        let state = store.quota_state().unwrap();
        let quota = FileQuota::new(2, 8199).unwrap();
        store.set_quota(state, quota).unwrap();
        let before = store.database.committed_wal().unwrap();
        let snapshot = store.capture().unwrap();
        assert_eq!(store.database.committed_wal().unwrap(), before);
        assert_eq!(snapshot.project(), PROJECT);
        assert_eq!(snapshot.quota(), quota);
        assert_eq!(snapshot.files(), std::slice::from_ref(&first));
        assert_eq!(
            snapshot.metadata_report().database_id,
            store.database.database_id()
        );
        assert_eq!(
            snapshot.metadata_report().wal_version,
            if compacted { 2 } else { 1 }
        );
        assert_eq!(
            snapshot.metadata_report().last_transaction,
            store.database.last_transaction()
        );
        assert_eq!(snapshot.objects().inventory().payload_bytes(), 8199);
        assert_eq!(snapshot.objects().objects().len(), 2);
        assert!(!format!("{snapshot:?}").contains("synthetic"));
        store.rename(FILE, first.revision(), "later").unwrap();
        drop(store);
        fs::remove_dir_all(temp.path().join("metadata")).unwrap();
        fs::remove_dir_all(temp.path().join("objects")).unwrap();
        let target = tempfile::tempdir().unwrap();
        let mut restored = component_round_trip(&snapshot, target.path());
        assert_eq!(restored.list().unwrap(), vec![first.clone()]);
        assert_eq!(restored.quota().unwrap(), quota);
        assert_eq!(
            restored.usage().unwrap(),
            FileUsage {
                physical_objects: 2,
                payload_bytes: 8199,
                references: 1,
                orphans: 1,
            }
        );
        let object = snapshot
            .objects()
            .objects()
            .iter()
            .find(|o| o.object() == OBJECT)
            .unwrap();
        assert_eq!(object.payload(), payload);
        let mut reader = restored.reader(FILE).unwrap();
        let mut copied = Vec::new();
        let mut scratch = [0; 1024];
        loop {
            let count = reader.read_payload(&mut scratch).unwrap();
            if count == 0 {
                break;
            }
            copied.extend_from_slice(&scratch[..count]);
        }
        reader.finish().unwrap();
        assert_eq!(copied, payload);
        restored
            .rename(FILE, first.revision(), "restored-write")
            .unwrap();
    }
}

#[test]
fn retained_actual_referenced_and_orphan_inodes_reject_same_byte_substitution_during_capture() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for orphan in [false, true] {
            for point in [
                CaptureBoundary::Admitted,
                CaptureBoundary::Metadata,
                CaptureBoundary::Objects,
            ] {
                let temp = tempfile::tempdir().unwrap();
                let mut store = setup(temp.path(), compacted);
                if orphan {
                    store.objects.put(OBJECT, b"synthetic").unwrap();
                } else {
                    store
                        .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
                        .unwrap();
                }
                let before = store.database.committed_wal().unwrap();
                let path = temp.path().join("objects").join(format!("{OBJECT}.object"));
                let saved = temp.path().join("original");
                let result = store.capture_with(|at| {
                    if at == point {
                        fs::rename(&path, &saved).unwrap();
                        fs::copy(&saved, &path).unwrap();
                    }
                });
                assert!(result.is_err());
                assert!(matches!(store.list(), Err(Error::Poisoned)));
                assert_eq!(fs::read(&path).unwrap(), fs::read(&saved).unwrap());
                assert_eq!(
                    fs::read(temp.path().join("metadata/redo.wal")).unwrap(),
                    before
                );
                drop(store);
                let store = open(temp.path());
                assert_eq!(store.usage().unwrap().orphans, usize::from(orphan));
            }
        }
    }
}

#[test]
fn moved_original_owners_supply_snapshot_without_adopting_old_path_replacements() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for owner in ["metadata", "objects"] {
            let temp = tempfile::tempdir().unwrap();
            let mut store = setup(temp.path(), compacted);
            store
                .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
                .unwrap();
            let original = store.database.committed_wal().unwrap();
            let saved = temp.path().join("original-owner");
            let path = temp.path().join(owner);
            let result = store.capture_with(|at| {
                if at == CaptureBoundary::Objects {
                    fs::rename(&path, &saved).unwrap();
                    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
                    for entry in fs::read_dir(&saved).unwrap() {
                        let entry = entry.unwrap();
                        fs::copy(entry.path(), path.join(entry.file_name())).unwrap();
                    }
                    if owner == "metadata" {
                        fs::write(path.join("redo.wal"), b"replacement-not-the-source").unwrap();
                    } else {
                        let image =
                            emilybase_object_storage::encode(PROJECT, OBJECT, b"replacement")
                                .unwrap();
                        fs::write(path.join(format!("{OBJECT}.object")), image).unwrap();
                    }
                }
            });
            let snapshot = result.unwrap();
            assert_eq!(
                &snapshot.metadata_bytes()[emilybase_backup::HEADER_SIZE..],
                original
            );
            assert_eq!(snapshot.objects().objects()[0].payload(), b"synthetic");
            assert_eq!(store.usage().unwrap().references, 1);
            assert!(saved.is_dir());
            assert!(path.is_dir());
            drop(store);
            if owner == "metadata" {
                assert!(Database::open(&path).is_err());
                assert!(Database::open(&saved).is_ok());
            } else {
                assert_eq!(
                    ProjectDirectory::open(&saved, PROJECT)
                        .unwrap()
                        .get(OBJECT)
                        .unwrap()
                        .payload(),
                    b"synthetic"
                );
                assert_eq!(
                    ProjectDirectory::open(&path, PROJECT)
                        .unwrap()
                        .get(OBJECT)
                        .unwrap()
                        .payload(),
                    b"replacement"
                );
            }
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn maximum_empty_orphan_capture_retains128_descriptors_and_releases_them_before_return() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), false);
    for index in 0..128 {
        store
            .objects
            .put(ObjectId::from_bytes([index; 16]), b"")
            .unwrap();
    }
    let before = fs::read_dir("/proc/self/fd").unwrap().count();
    let mut observations = 0;
    let snapshot = store
        .capture_with(|_| {
            assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), before + 128);
            assert!(Database::open(temp.path().join("metadata")).is_err());
            assert!(matches!(
                ProjectDirectory::open(temp.path().join("objects"), PROJECT),
                Err(emilybase_object_storage::Error::Busy)
            ));
            observations += 1;
        })
        .unwrap();
    assert_eq!(observations, 3);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), before);
    assert_eq!(snapshot.objects().objects().len(), 128);
    assert_eq!(snapshot.objects().inventory().payload_bytes(), 0);
    assert!(snapshot.files().is_empty());
    assert_eq!(store.usage().unwrap().orphans, 128);
}

#[test]
fn same_inode_source_rewrites_after_copy_refuse_without_repair_or_returned_image() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for metadata in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let mut store = setup(temp.path(), compacted);
            store
                .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
                .unwrap();
            let path = if metadata {
                temp.path().join("metadata/redo.wal")
            } else {
                temp.path().join("objects").join(format!("{OBJECT}.object"))
            };
            let before = fs::read(&path).unwrap();
            let mut changed = before.clone();
            if metadata {
                changed[0] ^= 0x80;
            } else {
                changed = emilybase_object_storage::encode(PROJECT, OBJECT, b"different").unwrap();
            }
            let result = store.capture_with(|at| {
                if at == CaptureBoundary::Objects {
                    fs::write(&path, &changed).unwrap();
                }
            });
            assert!(result.is_err());
            assert!(matches!(store.capture(), Err(Error::Poisoned)));
            assert_eq!(fs::read(&path).unwrap(), changed);
            assert_ne!(before, changed);
            drop(store);
            if metadata {
                assert!(Database::open(temp.path().join("metadata")).is_err());
            } else {
                let db = Database::open(temp.path().join("metadata")).unwrap();
                let objects = ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
                assert!(matches!(FileStore::open(db, objects), Err(Error::Corrupt)));
            }
        }
    }
}

#[test]
fn replayed_metadata_reuses_exact_live_schema_scope_and_revision_validation() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), false);
    let info = store
        .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
        .unwrap();
    let snapshot = store.capture().unwrap();
    let verified = emilybase_backup::decode_verified(snapshot.metadata_bytes()).unwrap();
    let image = verified.image();
    assert!(matches!(
        records::metadata_image(&image.snapshot, [9; 16], image.last_transaction, PROJECT),
        Err(Error::Scope)
    ));
    assert!(matches!(
        records::metadata_image(
            &image.snapshot,
            image.database_id,
            image.last_transaction,
            ProjectId::from_bytes([9; 16])
        ),
        Err(Error::Scope)
    ));
    assert!(matches!(
        records::metadata_image(
            &image.snapshot,
            image.database_id,
            info.revision() - 1,
            PROJECT
        ),
        Err(Error::Corrupt)
    ));
    let mut tx = store.database.begin().unwrap();
    let mut scope = records::scope_row(PROJECT, image.database_id, snapshot.quota());
    scope[1] = Value::Integer(2);
    tx.update(records::SCOPE, &Key::Integer(1), scope).unwrap();
    tx.commit().unwrap();
    assert!(store.capture().is_err());
    assert_eq!(snapshot.files(), &[info]);
    assert_eq!(snapshot.metadata_report(), verified.report());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_live_histories_capture_exact_independent_visibility_and_physical_payload_model(
        compacted in any::<bool>(), operations in prop::collection::vec((0u8..4, prop::collection::vec(any::<u8>(),0..257)),1..17),
    ) {
        let _serial = TEST_IO.lock().unwrap(); let temp=tempfile::tempdir().unwrap();
        let mut store=setup(temp.path(),compacted); let mut logical:Option<FileInfo>=None;
        let mut physical=Vec::new();
        for (step,(operation,payload)) in operations.into_iter().enumerate() {
            match operation {
                0 if logical.is_none()=>{
                    let object=ObjectId::from_bytes([step as u8+16;16]);
                    logical=Some(store.publish(FILE,object,[4;16],"synthetic",&payload).unwrap());
                    physical.push((object,payload));
                },
                1=>if let Some(info)=logical.take() {store.remove(FILE,info.revision()).unwrap();},
                2=>if let Some(info)=logical.clone() {logical=Some(store.rename(FILE,info.revision(),&format!("synthetic-{step}")).unwrap());},
                3=>{let state=store.quota_state().unwrap();store.set_quota(state,FileQuota::new(128,64*1024*1024-step as u64).unwrap()).unwrap();},
                _=>{},
            }
            let wal=store.database.committed_wal().unwrap();let snapshot=store.capture().unwrap();
            prop_assert_eq!(store.database.committed_wal().unwrap(),wal);
            prop_assert_eq!(snapshot.files(),logical.as_slice());
            prop_assert_eq!(snapshot.objects().objects().len(),physical.len());
            prop_assert_eq!(snapshot.objects().inventory().payload_bytes(),physical.iter().map(|(_,p)|p.len() as u64).sum::<u64>());
            for (id,payload) in &physical {
                let object=snapshot.objects().objects().iter().find(|o|o.object()==*id).unwrap();
                prop_assert_eq!(object.payload(),payload.as_slice());
            }
            let verified=emilybase_backup::decode_verified(snapshot.metadata_bytes()).unwrap();
            prop_assert_eq!(verified.image().last_transaction,store.database.last_transaction());
            drop(store);store=open(temp.path());
            prop_assert_eq!(store.list().unwrap(),logical.clone().into_iter().collect::<Vec<_>>());
        }
    }
}
