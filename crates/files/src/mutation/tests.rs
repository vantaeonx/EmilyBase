use super::*;
use crate::{FileQuota, FileUsage, TEST_IO};
use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
fn setup(path: &Path, compacted: bool) -> (FileStore, FileInfo) {
    let mut database = Database::create(path.join("metadata")).unwrap();
    if compacted {
        database.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(path.join("objects"), PROJECT).unwrap();
    let mut store =
        FileStore::initialize(database, objects, FileQuota::new(64, 4096).unwrap()).unwrap();
    let info = store
        .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
        .unwrap();
    (store, info)
}
fn reopen(path: &Path) -> FileStore {
    FileStore::open(
        Database::open(path.join("metadata")).unwrap(),
        ProjectDirectory::open(path.join("objects"), PROJECT).unwrap(),
    )
    .unwrap()
}
fn wal_version(store: &mut FileStore) -> u16 {
    emilybase_transactions::recover_image(
        &store.database.committed_wal().unwrap(),
        Some(store.database.database_id()),
    )
    .unwrap()
    .wal_version
}

#[test]
fn rename_and_logical_removal_preserve_immutable_bytes_scope_quota_and_exact_cas_on_both_wals() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut store, original) = setup(temp.path(), compacted);
        assert_eq!(wal_version(&mut store), if compacted { 2 } else { 1 });
        let path = temp.path().join("objects").join(format!("{OBJECT}.object"));
        let image = fs::read(&path).unwrap();
        let renamed = store
            .rename(FILE, original.revision(), "../display-only-новый.bin")
            .unwrap();
        assert_eq!(renamed.revision(), original.revision() + 1);
        assert_eq!(renamed.owner(), original.owner());
        assert_eq!(renamed.object(), original.object());
        assert_eq!(renamed.report(), original.report());
        assert_eq!(fs::read(&path).unwrap(), image);
        assert_eq!(
            store
                .rename(FILE, renamed.revision(), renamed.name())
                .unwrap(),
            renamed
        );
        assert_eq!(store.database.last_transaction(), renamed.revision());
        assert!(matches!(
            store.rename(FILE, original.revision(), renamed.name()),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            store.remove(FILE, original.revision()),
            Err(Error::Conflict)
        ));
        drop(store);
        let mut store = reopen(temp.path());
        let removed = store.remove(FILE, renamed.revision()).unwrap();
        assert_eq!(removed.removed(), &renamed);
        assert_eq!(removed.revision(), renamed.revision() + 1);
        assert!(!format!("{removed:?}").contains("display-only"));
        assert!(store.list().unwrap().is_empty());
        assert!(matches!(store.reader(FILE), Err(Error::Missing)));
        assert!(matches!(
            store.remove(FILE, renamed.revision()),
            Err(Error::Missing)
        ));
        assert_eq!(
            store.usage().unwrap(),
            FileUsage {
                physical_objects: 1,
                payload_bytes: 9,
                references: 0,
                orphans: 1
            }
        );
        assert_eq!(fs::read(&path).unwrap(), image);
        drop(store);
        let mut store = reopen(temp.path());
        let recreated = store
            .publish(
                FILE,
                ObjectId::from_bytes([5; 16]),
                [4; 16],
                "new-logical-generation",
                b"next",
            )
            .unwrap();
        assert!(recreated.revision() > removed.revision());
        assert!(matches!(
            store.remove(FILE, renamed.revision()),
            Err(Error::Conflict)
        ));
        assert_eq!(store.info(FILE).unwrap(), Some(recreated));
        assert_eq!(store.usage().unwrap().orphans, 1);
        assert_eq!(wal_version(&mut store), if compacted { 2 } else { 1 });
    }
}

#[test]
fn invalid_names_missing_stale_and_unchanged_requests_preserve_wal_and_inventory() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (mut store, original) = setup(temp.path(), false);
    let before = store.database.committed_wal().unwrap();
    let usage = store.usage().unwrap();
    for name in ["".to_string(), "bad\0name".into(), "x".repeat(257)] {
        assert!(matches!(
            store.rename(FILE, original.revision(), &name),
            Err(Error::Name)
        ));
    }
    for revision in [0, original.revision() - 1, u64::MAX] {
        assert!(matches!(
            store.rename(FILE, revision, "changed"),
            Err(Error::Conflict)
        ));
        assert!(matches!(store.remove(FILE, revision), Err(Error::Conflict)));
    }
    let absent = FileId::from_bytes([9; 16]);
    assert!(matches!(
        store.rename(absent, 1, "changed"),
        Err(Error::Missing)
    ));
    assert!(matches!(store.remove(absent, 1), Err(Error::Missing)));
    assert_eq!(
        store
            .rename(FILE, original.revision(), original.name())
            .unwrap(),
        original
    );
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert_eq!(store.usage().unwrap(), usage);
}

#[test]
fn source_identity_is_retained_before_and_after_the_actual_metadata_commit() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        for removing in [false, true] {
            for boundary in [MetadataBoundary::Staged, MetadataBoundary::Committed] {
                let temp = tempfile::tempdir().unwrap();
                let (mut store, original) = setup(temp.path(), compacted);
                let path = temp.path().join("objects").join(format!("{OBJECT}.object"));
                let saved = temp.path().join("original-inode");
                let result = store.mutate_with(
                    FILE,
                    original.revision(),
                    if removing { None } else { Some("changed") },
                    |at| {
                        if at == boundary {
                            fs::rename(&path, &saved).unwrap();
                            fs::copy(&saved, &path).unwrap();
                        }
                    },
                );
                if boundary == MetadataBoundary::Staged {
                    assert!(matches!(result, Err(Error::Objects(_))));
                    assert_eq!(store.database.last_transaction(), original.revision());
                } else {
                    assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
                    assert_eq!(store.database.last_transaction(), original.revision() + 1);
                }
                assert!(matches!(store.list(), Err(Error::Poisoned)));
                drop(store);
                let store = reopen(temp.path());
                let current = store.info(FILE).unwrap();
                if boundary == MetadataBoundary::Staged {
                    assert_eq!(current, Some(original));
                } else if removing {
                    assert!(current.is_none());
                    assert_eq!(store.usage().unwrap().orphans, 1);
                } else {
                    assert_eq!(current.unwrap().name(), "changed");
                }
                assert_eq!(fs::read(path).unwrap(), fs::read(saved).unwrap());
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_cas_remove_recreate_histories_match_independent_revision_and_physical_usage_model(
        compacted in any::<bool>(),operations in prop::collection::vec(0u8..5,1..32),
    ) {
        let _serial=TEST_IO.lock().unwrap();
        let temp=tempfile::tempdir().unwrap();let (mut store,initial)=setup(temp.path(),compacted);
        let mut logical=Some(initial);let mut revision=store.database.last_transaction();let mut physical=1;
        for (step,operation) in operations.into_iter().enumerate() {
            match (operation,logical.clone()) {
                (0,Some(info))=>{
                    let name=format!("synthetic-{}",step%3);
                    let changed=store.rename(FILE,info.revision(),&name).unwrap();
                    if name!=info.name() {revision+=1;}
                    prop_assert_eq!(changed.revision(),revision);prop_assert_eq!(changed.name(),name.as_str());
                    logical=Some(changed);
                }
                (1,Some(_))=>prop_assert!(matches!(store.rename(FILE,0,"stale"),Err(Error::Conflict))),
                (2,Some(info))=>{
                    let removed=store.remove(FILE,info.revision()).unwrap();revision+=1;
                    prop_assert_eq!(removed.revision(),revision);logical=None;
                }
                (3,Some(_))=>prop_assert!(matches!(store.remove(FILE,0),Err(Error::Conflict))),
                (4,None)=>{
                    let object=ObjectId::from_bytes([physical as u8+16;16]);
                    let created=store.publish(FILE,object,[4;16],"synthetic",b"synthetic").unwrap();
                    physical+=1;revision+=1;prop_assert_eq!(created.revision(),revision);logical=Some(created);
                }
                (_,None)=>prop_assert!(matches!(store.remove(FILE,0),Err(Error::Missing))),
                _=>{},
            }
            prop_assert_eq!(store.database.last_transaction(),revision);
            drop(store);store=reopen(temp.path());
            prop_assert_eq!(store.info(FILE).unwrap(),logical.clone());
            prop_assert_eq!(store.usage().unwrap(),FileUsage {physical_objects:physical,payload_bytes:physical as u64*9,
                references:usize::from(logical.is_some()),orphans:physical-usize::from(logical.is_some())});
            prop_assert_eq!(wal_version(&mut store),if compacted {2}else{1});
        }
    }
}
