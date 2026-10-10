use super::*;
use crate::{FileId, FileUsage, TEST_IO};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
fn setup(path: &Path, compacted: bool, quota: FileQuota) -> FileStore {
    let mut database = Database::create(path.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(path.join("objects"), PROJECT).unwrap();
    FileStore::initialize(database, objects, quota).unwrap()
}
fn reopen(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap()
}

#[test]
fn scope_global_revision_noop_and_orphan_minimum_persist_on_both_native_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut store = setup(temp.path(), compacted, FileQuota::new(4, 100).unwrap());
        let original = store.quota_state().unwrap();
        assert_eq!(original.project(), PROJECT);
        assert_eq!(original.database_id(), &store.database.database_id());
        let closed = store
            .set_quota(original, FileQuota::new(2, 0).unwrap())
            .unwrap();
        assert_eq!(closed.revision(), original.revision() + 1);
        let before = store.database.committed_wal().unwrap();
        assert_eq!(store.set_quota(closed, closed.quota()).unwrap(), closed);
        assert_eq!(store.database.committed_wal().unwrap(), before);
        assert!(matches!(
            store.set_quota(original, closed.quota()),
            Err(Error::Conflict)
        ));
        assert!(
            store
                .publish(FILE, OBJECT, [4; 16], "synthetic", b"payload")
                .is_err()
        );
        let opened = store
            .set_quota(closed, FileQuota::new(2, 20).unwrap())
            .unwrap();
        let info = store
            .publish(FILE, OBJECT, [4; 16], "synthetic", b"payload")
            .unwrap();
        assert!(matches!(
            store.set_quota(opened, opened.quota()),
            Err(Error::Conflict)
        ));
        let before_rename = store.quota_state().unwrap();
        let info = store.rename(FILE, info.revision(), "changed").unwrap();
        assert!(matches!(
            store.set_quota(before_rename, before_rename.quota()),
            Err(Error::Conflict)
        ));
        let before_remove = store.quota_state().unwrap();
        store.remove(FILE, info.revision()).unwrap();
        assert!(matches!(
            store.set_quota(before_remove, before_remove.quota()),
            Err(Error::Conflict)
        ));
        let current = store.quota_state().unwrap();
        assert!(matches!(
            store.set_quota(current, FileQuota::new(0, 0).unwrap()),
            Err(Error::Quota)
        ));
        assert!(matches!(
            store.set_quota(current, FileQuota::new(1, 6).unwrap()),
            Err(Error::Quota)
        ));
        let exact = store
            .set_quota(current, FileQuota::new(1, 7).unwrap())
            .unwrap();
        drop(store);
        let store = reopen(temp.path());
        assert_eq!(store.quota_state().unwrap(), exact);
        assert_eq!(
            store.usage().unwrap(),
            FileUsage {
                physical_objects: 1,
                payload_bytes: 7,
                references: 0,
                orphans: 1
            }
        );
    }
}

#[test]
fn copied_other_database_scope_and_forged_snapshot_metadata_cannot_bypass_cas() {
    let _serial = TEST_IO.lock().unwrap();
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let a = setup(first.path(), false, FileQuota::new(2, 100).unwrap());
    let mut b = setup(second.path(), false, FileQuota::new(2, 100).unwrap());
    let state = a.quota_state().unwrap();
    let current = b.quota_state().unwrap();
    assert_eq!(state.revision(), current.revision());
    assert!(matches!(
        b.set_quota(state, FileQuota::new(1, 10).unwrap()),
        Err(Error::Scope)
    ));
    for shape in 0..4 {
        let mut bad = current;
        match shape {
            0 => bad.project = ProjectId::from_bytes([9; 16]),
            1 => bad.database = [9; 16],
            2 => bad.revision = 0,
            _ => bad.quota = FileQuota::new(1, 10).unwrap(),
        }
        assert!(b.set_quota(bad, FileQuota::new(1, 10).unwrap()).is_err());
        assert_eq!(b.quota_state().unwrap(), current);
    }
    assert!(!format!("{current:?}").contains(&PROJECT.to_string()));
}

#[test]
fn all_original_blob_descriptors_including_orphans_span_quota_commit_on_both_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for orphan in [false, true] {
            for point in [MetadataBoundary::Staged, MetadataBoundary::Committed] {
                let temp = tempfile::tempdir().unwrap();
                let initial = FileQuota::new(4, 1024).unwrap();
                let mut store = setup(temp.path(), compacted, initial);
                if orphan {
                    store.objects.put(OBJECT, b"synthetic").unwrap();
                } else {
                    store
                        .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
                        .unwrap();
                }
                let state = store.quota_state().unwrap();
                let next = FileQuota::new(3, 512).unwrap();
                let path = temp.path().join("objects").join(format!("{OBJECT}.object"));
                let saved = temp.path().join("actual-original");
                let result = store.set_quota_with(state, next, |at| {
                    if at == point {
                        fs::rename(&path, &saved).unwrap();
                        fs::copy(&saved, &path).unwrap();
                    }
                });
                if point == MetadataBoundary::Staged {
                    assert!(matches!(result, Err(Error::Objects(_))));
                    assert_eq!(store.database.last_transaction(), state.revision());
                } else {
                    assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
                }
                assert!(matches!(store.quota_state(), Err(Error::Poisoned)));
                drop(store);
                let store = reopen(temp.path());
                assert_eq!(
                    store.quota().unwrap(),
                    if point == MetadataBoundary::Staged {
                        initial
                    } else {
                        next
                    }
                );
                assert_eq!(store.usage().unwrap().orphans, usize::from(orphan));
                assert_eq!(fs::read(path).unwrap(), fs::read(saved).unwrap());
            }
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn maximum_all_orphan_inventory_retains_bounded_handles_and_closes_them_after_update() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), false, FileQuota::new(128, 0).unwrap());
    for index in 0..128 {
        store
            .objects
            .put(ObjectId::from_bytes([index; 16]), b"")
            .unwrap();
    }
    let before = fs::read_dir("/proc/self/fd").unwrap().count();
    let mut peak = 0;
    let state = store.quota_state().unwrap();
    let changed = store
        .set_quota_with(state, FileQuota::new(128, 1).unwrap(), |point| {
            if point == MetadataBoundary::Staged {
                peak = fs::read_dir("/proc/self/fd").unwrap().count();
            }
        })
        .unwrap();
    assert_eq!(peak, before + 128);
    assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), before);
    assert!(matches!(
        store.set_quota(changed, FileQuota::new(127, 1).unwrap()),
        Err(Error::Quota)
    ));
    drop(store);
    let store = reopen(temp.path());
    assert_eq!(store.usage().unwrap().orphans, 128);
    assert_eq!(store.quota_state().unwrap(), changed);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_quota_and_reference_histories_match_independent_global_cas_and_charge_model(
        compacted in any::<bool>(),operations in prop::collection::vec((0u8..4,0usize..9,0u64..101,any::<bool>()),1..32),
    ) {
        let _serial=TEST_IO.lock().unwrap();let temp=tempfile::tempdir().unwrap();
        let mut quota=FileQuota::new(4,100).unwrap();let mut store=setup(temp.path(),compacted,quota);
        let mut revision=store.database.last_transaction();let mut physical=0;let mut logical=None;
        let mut cached=store.quota_state().unwrap();
        for (step,(operation,objects,bytes,stale)) in operations.into_iter().enumerate() {
            let issued=store.quota_state().unwrap();
            match operation {
                0=>{
                    let expected=if stale {cached}else{issued};let candidate=FileQuota::new(objects,bytes).unwrap();
                    let result=store.set_quota(expected,candidate);
                    if expected.revision()!=revision {prop_assert!(matches!(result,Err(Error::Conflict)));}
                    else if physical>objects || physical as u64*9>bytes {prop_assert!(matches!(result,Err(Error::Quota)));}
                    else {if candidate!=quota {revision+=1;quota=candidate;}prop_assert_eq!(result.unwrap().revision(),revision);}
                }
                1 if logical.is_none()=>{
                    let result=store.publish(FILE,ObjectId::from_bytes([physical as u8+16;16]),[4;16],"synthetic",b"synthetic");
                    if physical<quota.objects() && (physical as u64+1)*9<=quota.payload_bytes() {
                        revision+=1;physical+=1;let info=result.unwrap();prop_assert_eq!(info.revision(),revision);logical=Some(info);
                    } else {prop_assert!(result.is_err());}
                }
                2=>if let Some(info)=logical.clone() {
                    store.remove(FILE,info.revision()).unwrap();revision+=1;logical=None;
                },
                3=>if let Some(info)=logical.clone() {
                    let name=format!("synthetic-{step}");let changed=store.rename(FILE,info.revision(),&name).unwrap();
                    revision+=1;logical=Some(changed);
                },
                _=>{},
            }
            cached=issued;drop(store);store=reopen(temp.path());
            prop_assert_eq!(store.database.last_transaction(),revision);prop_assert_eq!(store.quota().unwrap(),quota);
            prop_assert_eq!(store.info(FILE).unwrap(),logical.clone());
            prop_assert_eq!(store.usage().unwrap(),FileUsage {physical_objects:physical,payload_bytes:physical as u64*9,
                references:usize::from(logical.is_some()),orphans:physical-usize::from(logical.is_some())});
        }
    }
}
