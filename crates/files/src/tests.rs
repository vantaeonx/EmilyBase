use super::*;
use crate::{MAX_FILE_NAME_BYTES, TEST_IO};
use emilybase_catalog::{Key, Value};
use emilybase_object_storage::{MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, MAX_PAYLOAD_BYTES};
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([0xab; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
fn owners(parent: &Path) -> (Database, ProjectDirectory) {
    let db = Database::create(parent.join("metadata")).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(parent.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(parent.join("objects"), PROJECT).unwrap();
    (db, objects)
}
fn setup(parent: &Path, quota: FileQuota) -> FileStore {
    let (db, objects) = owners(parent);
    FileStore::initialize(db, objects, quota).unwrap()
}
fn reopen(parent: &Path) -> Result<FileStore> {
    let db = Database::open(parent.join("metadata"))?;
    let objects = ProjectDirectory::open(parent.join("objects"), PROJECT)?;
    FileStore::open(db, objects)
}
fn all_bytes(mut reader: ObjectReader<'_>) -> Vec<u8> {
    let mut result = Vec::new();
    let mut scratch = [0; 8192];
    loop {
        let n = reader.read_payload(&mut scratch).unwrap();
        if n == 0 {
            break;
        }
        result.extend_from_slice(&scratch[..n]);
    }
    reader.finish().unwrap();
    result
}

#[test]
fn original_wal_reference_quota_scope_and_payload_survive_independent_reopen() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quota = FileQuota::new(3, 1024).unwrap();
    let mut store = setup(temp.path(), quota);
    assert_eq!(store.project(), PROJECT);
    assert!(store.list().unwrap().is_empty());
    let info = store
        .publish(
            FILE,
            OBJECT,
            [4; 16],
            "../../synthetic-данные.bin",
            b"synthetic\0\xff",
        )
        .unwrap();
    assert_eq!(info.id(), FILE);
    assert_eq!(info.object(), OBJECT);
    assert_eq!(info.owner(), &[4; 16]);
    assert_eq!(info.name(), "../../synthetic-данные.bin");
    assert_eq!(info.report().payload_bytes, 11);
    assert_eq!(info.revision(), store.database.last_transaction());
    assert!(!format!("{info:?}").contains("synthetic"));
    assert!(!temp.path().join("synthetic-данные.bin").exists());
    assert_eq!(all_bytes(store.reader(FILE).unwrap()), b"synthetic\0\xff");
    assert_eq!(store.info(FILE).unwrap(), Some(info.clone()));
    assert!(store.info(FileId::from_bytes([9; 16])).unwrap().is_none());
    assert!(matches!(
        store.reader(FileId::from_bytes([9; 16])),
        Err(Error::Missing)
    ));
    assert_eq!(
        store.usage().unwrap(),
        FileUsage {
            physical_objects: 1,
            payload_bytes: 11,
            references: 1,
            orphans: 0
        }
    );
    assert!(Database::open(temp.path().join("metadata")).is_err());
    assert!(matches!(
        ProjectDirectory::open(temp.path().join("objects"), PROJECT),
        Err(emilybase_object_storage::Error::Busy)
    ));
    drop(store);
    let store = reopen(temp.path()).unwrap();
    assert_eq!(store.quota().unwrap(), quota);
    assert_eq!(store.list().unwrap(), vec![info]);
    assert_eq!(all_bytes(store.reader(FILE).unwrap()), b"synthetic\0\xff");
}

#[test]
fn invalid_inputs_duplicate_ids_and_persisted_capacity_refuse_before_publication_or_commit() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), FileQuota::new(2, 9).unwrap());
    let revision = store.database.last_transaction();
    for name in [
        "".to_string(),
        "bad\0name".into(),
        "bad\r\nname".into(),
        "a".repeat(MAX_FILE_NAME_BYTES + 1),
    ] {
        assert!(matches!(
            store.publish(FILE, OBJECT, [4; 16], &name, b"synthetic"),
            Err(Error::Name)
        ));
        assert_eq!(store.database.last_transaction(), revision);
        assert_eq!(store.usage().unwrap().physical_objects, 0);
    }
    assert!(
        store
            .publish(
                FILE,
                OBJECT,
                [4; 16],
                "valid",
                &vec![0; MAX_PAYLOAD_BYTES + 1]
            )
            .is_err()
    );
    let info = store
        .publish(FILE, OBJECT, [4; 16], "valid", b"synthetic")
        .unwrap();
    assert!(matches!(
        store.publish(
            FILE,
            ObjectId::from_bytes([5; 16]),
            [4; 16],
            "duplicate",
            b""
        ),
        Err(Error::Exists)
    ));
    assert!(
        store
            .publish(
                FileId::from_bytes([5; 16]),
                OBJECT,
                [4; 16],
                "duplicate-blob",
                b""
            )
            .is_err()
    );
    assert!(
        store
            .publish(
                FileId::from_bytes([5; 16]),
                ObjectId::from_bytes([5; 16]),
                [4; 16],
                "too-many-bytes",
                b"x"
            )
            .is_err()
    );
    assert_eq!(store.database.last_transaction(), info.revision());
    assert_eq!(store.usage().unwrap().physical_objects, 1);
    let empty = store
        .publish(
            FileId::from_bytes([5; 16]),
            ObjectId::from_bytes([5; 16]),
            [4; 16],
            "empty",
            b"",
        )
        .unwrap();
    assert!(
        store
            .publish(
                FileId::from_bytes([6; 16]),
                ObjectId::from_bytes([6; 16]),
                [4; 16],
                "too-many-objects",
                b""
            )
            .is_err()
    );
    assert_eq!(store.database.last_transaction(), empty.revision());
    assert!(FileQuota::new(MAX_INVENTORY_OBJECTS + 1, 0).is_err());
    assert!(FileQuota::new(0, MAX_INVENTORY_BYTES + 1).is_err());
    let text = FILE.to_string();
    assert_eq!(text.parse::<FileId>().unwrap(), FILE);
    for invalid in [
        text.to_uppercase(),
        "../escape".into(),
        "0".repeat(31),
        "0".repeat(33),
    ] {
        assert!(invalid.parse::<FileId>().is_err());
    }
}

#[test]
fn pristine_initialization_refuses_existing_history_and_valid_unreferenced_blobs() {
    let _serial = TEST_IO.lock().unwrap();
    let quota = FileQuota::new(2, 1024).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (db, mut objects) = owners(temp.path());
    objects.put(OBJECT, b"orphan").unwrap();
    assert!(matches!(
        FileStore::initialize(db, objects, quota),
        Err(Error::NotEmpty)
    ));
    assert_eq!(
        Database::open(temp.path().join("metadata"))
            .unwrap()
            .view()
            .unwrap()
            .event_count(),
        1
    );
    let temp = tempfile::tempdir().unwrap();
    let (mut db, objects) = owners(temp.path());
    let mut tx = db.begin().unwrap();
    tx.create_table(records::scope_schema()).unwrap();
    tx.drop_table(records::SCOPE).unwrap();
    tx.commit().unwrap();
    assert!(matches!(
        FileStore::initialize(db, objects, quota),
        Err(Error::NotEmpty)
    ));
}

#[test]
fn unreferenced_valid_blobs_are_invisible_but_consume_persisted_physical_capacity_after_restart() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), FileQuota::new(2, 14).unwrap());
    store.objects.put(OBJECT, b"orphan").unwrap();
    drop(store);
    let mut store = reopen(temp.path()).unwrap();
    assert!(store.list().unwrap().is_empty());
    assert!(matches!(
        store.reader(FileId::from_bytes(*OBJECT.as_bytes())),
        Err(Error::Missing)
    ));
    assert_eq!(
        store.usage().unwrap(),
        FileUsage {
            physical_objects: 1,
            payload_bytes: 6,
            references: 0,
            orphans: 1
        }
    );
    let other = ObjectId::from_bytes([5; 16]);
    store
        .publish(FILE, other, [4; 16], "live", b"12345678")
        .unwrap();
    assert!(
        store
            .publish(
                FileId::from_bytes([6; 16]),
                ObjectId::from_bytes([6; 16]),
                [4; 16],
                "full",
                b""
            )
            .is_err()
    );
    drop(store);
    let store = reopen(temp.path()).unwrap();
    assert_eq!(
        store.usage().unwrap(),
        FileUsage {
            physical_objects: 2,
            payload_bytes: 14,
            references: 1,
            orphans: 1
        }
    );
    assert_eq!(all_bytes(store.reader(FILE).unwrap()), b"12345678");
}

#[test]
fn zero_quota_is_persisted_closed_and_admission_never_accepts_new_per_call_limits() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), FileQuota::new(0, 0).unwrap());
    assert!(store.publish(FILE, OBJECT, [4; 16], "empty", b"").is_err());
    drop(store);
    let mut store = reopen(temp.path()).unwrap();
    assert_eq!(store.quota().unwrap(), FileQuota::new(0, 0).unwrap());
    assert!(store.publish(FILE, OBJECT, [4; 16], "empty", b"").is_err());
    assert_eq!(store.usage().unwrap().physical_objects, 0);
}

#[test]
fn maximum_reference_count_and_payload_fit_original_metadata_and_exact_persisted_capacity() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(
        temp.path(),
        FileQuota::new(MAX_INVENTORY_OBJECTS, 0).unwrap(),
    );
    for index in 0..MAX_INVENTORY_OBJECTS {
        store
            .publish(
                FileId::from_bytes([index as u8; 16]),
                ObjectId::from_bytes([index as u8; 16]),
                [4; 16],
                "synthetic-empty",
                b"",
            )
            .unwrap();
    }
    assert!(
        store
            .publish(
                FileId::from_bytes([200; 16]),
                ObjectId::from_bytes([200; 16]),
                [4; 16],
                "full",
                b""
            )
            .is_err()
    );
    drop(store);
    let store = reopen(temp.path()).unwrap();
    assert_eq!(store.list().unwrap().len(), MAX_INVENTORY_OBJECTS);
    assert_eq!(
        store.usage().unwrap().physical_objects,
        MAX_INVENTORY_OBJECTS
    );
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(
        temp.path(),
        FileQuota::new(2, MAX_PAYLOAD_BYTES as u64).unwrap(),
    );
    let payload = vec![0x73; MAX_PAYLOAD_BYTES];
    let expected = store
        .publish(FILE, OBJECT, [4; 16], &"я".repeat(128), &payload)
        .unwrap();
    assert!(
        store
            .publish(
                FileId::from_bytes([5; 16]),
                ObjectId::from_bytes([5; 16]),
                [4; 16],
                "over",
                b"x"
            )
            .is_err()
    );
    drop(store);
    let store = reopen(temp.path()).unwrap();
    assert_eq!(all_bytes(store.reader(FILE).unwrap()), payload);
    assert_eq!(store.info(FILE).unwrap(), Some(expected));
}

#[test]
#[ignore = "explicit synthetic corpus generator for the file_reference sanitizer target"]
fn file_reference_fuzz_seeds() {
    let Some(path) = std::env::var_os("EMILYBASE_FILE_RECORD_CORPUS") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    fs::create_dir_all(&path).unwrap();
    let info = FileInfo {
        id: FILE,
        object: OBJECT,
        owner: [4; 16],
        name: "synthetic".into(),
        report: emilybase_object_storage::FileReport {
            payload_bytes: 9,
            sha256: [5; 32],
        },
        revision: 3,
    };
    for shape in 0..18 {
        let mut row = records::encode(&info);
        match shape {
            1 => row[0] = Value::Text(FILE.to_string().to_uppercase()),
            2 => row[1] = Value::Bytes(vec![3; 15]),
            3 => row[2] = Value::Bytes(vec![4; 17]),
            4 => row[3] = Value::Text("".into()),
            5 => row[3] = Value::Text("bad\0name".into()),
            6 => row[3] = Value::Text("я".repeat(128)),
            7 => row[3] = Value::Text("a".repeat(257)),
            8 => row[4] = Value::Integer(-1),
            9 => row[4] = Value::Integer(MAX_PAYLOAD_BYTES as i64),
            10 => row[4] = Value::Integer(MAX_PAYLOAD_BYTES as i64 + 1),
            11 => row[5] = Value::Bytes(vec![5; 31]),
            12 => row[6] = Value::Bytes(0u64.to_le_bytes().to_vec()),
            13 => row[6] = Value::Bytes(u64::MAX.to_le_bytes().to_vec()),
            14 => row[6] = Value::Bytes(vec![3; 7]),
            15 => row.push(Value::Null),
            16 => {
                row.pop();
            }
            17 => row[4] = Value::Float(9.0),
            _ => {}
        }
        for last in [0u64, 2, 3, u64::MAX] {
            let mut bytes = last.to_le_bytes().to_vec();
            bytes.extend_from_slice(&emilybase_catalog::encode_row(&row).unwrap());
            fs::write(path.join(format!("shape-{shape}-last-{last}")), bytes).unwrap();
        }
    }
}

#[test]
fn incomplete_or_semantically_corrupt_graph_refuses_open_without_repair_or_orphan_adoption() {
    let _serial = TEST_IO.lock().unwrap();
    for shape in 0..15 {
        let temp = tempfile::tempdir().unwrap();
        let mut store = setup(temp.path(), FileQuota::new(3, 1024).unwrap());
        let info = store
            .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
            .unwrap();
        let mut row = records::encode(&info);
        let identity = store.database.database_id();
        let mut tx = store.database.begin().unwrap();
        if shape < 6 {
            let mut scope = records::scope_row(PROJECT, identity, FileQuota::new(3, 1024).unwrap());
            match shape {
                0 => scope[1] = Value::Integer(2),
                1 => scope[2] = Value::Bytes(vec![9; 16]),
                2 => scope[3] = Value::Bytes(vec![9; 16]),
                3 => scope[4] = Value::Integer(-1),
                4 => scope[5] = Value::Integer(-1),
                _ => scope[4] = Value::Integer(129),
            }
            tx.update(records::SCOPE, &Key::Integer(1), scope).unwrap();
        } else {
            match shape {
                6 => row[1] = Value::Bytes(vec![9; 16]),
                7 => row[2] = Value::Bytes(vec![4; 15]),
                8 => row[3] = Value::Text("bad\0name".into()),
                9 => row[4] = Value::Integer(-1),
                10 => row[5] = Value::Bytes(vec![0; 32]),
                11 => row[6] = Value::Bytes(vec![0; 8]),
                12 => row[6] = Value::Bytes(u64::MAX.to_le_bytes().to_vec()),
                13 => row[5] = Value::Bytes(vec![0; 31]),
                _ => row[0] = Value::Text(FileId::from_bytes([9; 16]).to_string()),
            }
            if shape == 14 {
                tx.insert(records::FILES, row).unwrap();
            } else {
                tx.update(records::FILES, &Key::Text(FILE.to_string()), row)
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        drop(store);
        let blob = temp.path().join("objects").join(format!("{OBJECT}.object"));
        let before = fs::read(&blob).unwrap();
        assert!(reopen(temp.path()).is_err(), "shape {shape}");
        assert_eq!(fs::read(blob).unwrap(), before);
    }
}

#[test]
fn wrong_metadata_database_identity_scope_and_foreign_layout_refuse_attachment() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (mut db, objects) = owners(temp.path());
    let identity = db.database_id();
    let mut tx = db.begin().unwrap();
    tx.create_table(records::scope_schema()).unwrap();
    tx.create_table(records::file_schema()).unwrap();
    tx.insert(
        records::SCOPE,
        records::scope_row(
            ProjectId::from_bytes([9; 16]),
            identity,
            FileQuota::new(1, 0).unwrap(),
        ),
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(matches!(FileStore::open(db, objects), Err(Error::Scope)));
    let temp = tempfile::tempdir().unwrap();
    let (mut db, objects) = owners(temp.path());
    let mut tx = db.begin().unwrap();
    tx.create_table(records::scope_schema()).unwrap();
    tx.commit().unwrap();
    assert!(matches!(FileStore::open(db, objects), Err(Error::Corrupt)));
}

#[test]
fn original_selected_inode_spans_metadata_commit_and_unknown_outcome_requires_reopen() {
    let _serial = TEST_IO.lock().unwrap();
    for boundary in [PublishBoundary::Blob, PublishBoundary::Metadata] {
        let temp = tempfile::tempdir().unwrap();
        let mut store = setup(temp.path(), FileQuota::new(3, 1024).unwrap());
        let path = temp.path().join("objects").join(format!("{OBJECT}.object"));
        let saved = temp.path().join("actual-selected");
        let result = store.publish_with(FILE, OBJECT, [4; 16], "synthetic", b"synthetic", |at| {
            if at == boundary {
                fs::rename(&path, &saved).unwrap();
                fs::copy(&saved, &path).unwrap();
            }
        });
        assert!(matches!(result, Err(Error::OutcomeUnknown(_))));
        assert!(matches!(store.list(), Err(Error::Poisoned)));
        assert!(matches!(store.reader(FILE), Err(Error::Poisoned)));
        drop(store);
        let store = reopen(temp.path()).unwrap();
        assert_eq!(
            store.usage().unwrap().references,
            usize::from(boundary == PublishBoundary::Metadata)
        );
        assert_eq!(
            store.usage().unwrap().orphans,
            usize::from(boundary == PublishBoundary::Blob)
        );
        assert_eq!(fs::read(path).unwrap(), fs::read(saved).unwrap());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_durable_references_match_independent_bounded_model_across_each_reopen(
        payloads in prop::collection::vec(prop::collection::vec(any::<u8>(),0..256),0..8),
    ) {
        let _serial=TEST_IO.lock().unwrap();
        let temp=tempfile::tempdir().unwrap();let mut store=setup(temp.path(),FileQuota::new(8,2048).unwrap());
        let mut expected=std::collections::BTreeMap::new();let mut bytes=0;
        for (index,payload) in payloads.iter().enumerate() {
            let id=FileId::from_bytes([index as u8;16]);let object=ObjectId::from_bytes([index as u8+64;16]);
            let name=format!("synthetic-{index}");let info=store.publish(id,object,[index as u8;16],&name,payload).unwrap();
            bytes+=payload.len() as u64;expected.insert(id,(info,payload.clone()));
            drop(store);store=reopen(temp.path()).unwrap();
            prop_assert_eq!(store.list().unwrap(),expected.values().map(|(i,_)|i.clone()).collect::<Vec<_>>());
            prop_assert_eq!(store.usage().unwrap(),FileUsage {physical_objects:expected.len(),payload_bytes:bytes,references:expected.len(),orphans:0});
            for (id,(_,body)) in &expected {prop_assert_eq!(&all_bytes(store.reader(*id).unwrap()),body);}
        }
    }
}
