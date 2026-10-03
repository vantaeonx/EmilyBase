use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
use emilybase_transactions::{
    Database, INDEX_IMAGE_HEADER, MAX_INDEX_IMAGE_BYTES, inspect_primary_index_image,
};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;

fn schema(name: &str, kind: DataType) -> Schema {
    Schema {
        name: name.into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: kind,
                nullable: false,
            },
            Column {
                name: "n".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}
fn initialized(path: &std::path::Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    for name in ["t", "s"] {
        transaction
            .create_table(schema(name, DataType::Integer))
            .unwrap();
        for id in 0..3 {
            transaction
                .insert(name, vec![Value::Integer(id), Value::Integer(id * 7)])
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    database
}
fn seal(bytes: &mut [u8]) {
    let payload = bytes.len() - INDEX_IMAGE_HEADER;
    bytes[80..88].copy_from_slice(&(payload as u64).to_le_bytes());
    let hash = Sha256::digest(&bytes[INDEX_IMAGE_HEADER..]);
    bytes[88..120].copy_from_slice(&hash);
    let checksum = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&checksum.to_le_bytes());
}
fn with_tree(bytes: &[u8], tree: BPlusTree, revision: u64) -> Vec<u8> {
    let mut result = bytes[..INDEX_IMAGE_HEADER].to_vec();
    result.extend_from_slice(&IndexSnapshot { revision, tree }.encode().unwrap());
    result[32..40].copy_from_slice(&revision.to_le_bytes());
    seal(&mut result);
    result
}

#[test]
fn image_header_nested_topology_and_hydration_preserve_exact_acknowledged_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    let before = database.committed_wal().unwrap();
    let fingerprint = database.view().unwrap().page_fingerprint();
    let bytes = database.primary_index_image("t").unwrap();
    assert_eq!(&bytes[..8], b"EBTI\0\0\0\0");
    assert_eq!(&bytes[8..12], &[1, 0, 128, 0]);
    assert_eq!(&bytes[16..32], &database.database_id());
    assert_eq!(&bytes[32..40], &database.last_transaction().to_le_bytes());
    assert_eq!(&bytes[40..48], &1u64.to_le_bytes());
    assert_eq!(&bytes[48..80], &fingerprint);
    assert!(
        bytes[12..16]
            .iter()
            .chain(&bytes[120..124])
            .all(|byte| *byte == 0)
    );
    let report = inspect_primary_index_image(&bytes).unwrap();
    assert_eq!(
        report,
        database.verify_primary_index_image("t", &bytes).unwrap()
    );
    assert_eq!((report.entries, report.pages, report.table_id), (3, 1, 1));
    let old = database.view().unwrap().clone();
    let info = database.load_primary_index_image("t", &bytes).unwrap();
    assert_eq!(info.entries, 3);
    assert!(
        database
            .view()
            .unwrap()
            .export_primary_tree("t")
            .unwrap()
            .has_stable_ids()
    );
    assert_eq!(database.committed_wal().unwrap(), before);
    assert_eq!(database.view().unwrap().page_fingerprint(), fingerprint);
    assert_eq!(
        database.view().unwrap().get("t", &Key::Integer(2)).unwrap(),
        old.get("t", &Key::Integer(2)).unwrap()
    );
    let mut transaction = database.begin().unwrap();
    transaction.delete("t", &Key::Integer(1)).unwrap();
    transaction.commit().unwrap();
    assert!(database.verify_primary_index_image("t", &bytes).is_err());
    assert!(
        old.verify_primary_tree(
            "t",
            &IndexSnapshot::decode(&bytes[INDEX_IMAGE_HEADER..])
                .unwrap()
                .tree
        )
        .is_ok()
    );
}

#[test]
fn identical_rows_foreign_database_and_table_images_require_exact_binding() {
    let dir = tempfile::tempdir().unwrap();
    let source = initialized(&dir.path().join("a"));
    let mut other = initialized(&dir.path().join("b"));
    assert_eq!(
        source.view().unwrap().page_fingerprint(),
        other.view().unwrap().page_fingerprint()
    );
    let image = source.primary_index_image("t").unwrap();
    assert!(inspect_primary_index_image(&image).is_ok());
    let before = other.committed_wal().unwrap();
    assert!(other.load_primary_index_image("t", &image).is_err());
    assert_eq!(other.committed_wal().unwrap(), before);
    assert!(source.verify_primary_index_image("s", &image).is_err());
    assert!(
        source
            .verify_primary_index_image("missing", &image)
            .is_err()
    );
}

#[test]
fn rollback_and_noop_keep_images_current_but_any_committed_history_change_retires_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = initialized(&dir.path().join("db"));
    let image = database.primary_index_image("t").unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .update(
            "t",
            &Key::Integer(0),
            vec![Value::Integer(0), Value::Integer(99)],
        )
        .unwrap();
    let speculative = transaction
        .view()
        .unwrap()
        .export_primary_tree("t")
        .unwrap();
    transaction.rollback();
    let forged = with_tree(&image, speculative, database.last_transaction());
    assert!(inspect_primary_index_image(&forged).is_ok());
    assert!(database.load_primary_index_image("t", &forged).is_err());
    assert_eq!(database.primary_index_image("t").unwrap(), image);
    database.begin().unwrap().commit().unwrap();
    assert!(database.verify_primary_index_image("t", &image).is_ok());
    let mut transaction = database.begin().unwrap();
    transaction
        .update(
            "s",
            &Key::Integer(0),
            vec![Value::Integer(0), Value::Integer(9)],
        )
        .unwrap();
    transaction.commit().unwrap();
    assert!(database.verify_primary_index_image("t", &image).is_err());
}

#[test]
fn semantic_repair_of_checksums_cannot_hydrate_wrong_keys_pointers_or_acknowledged_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = initialized(&dir.path().join("db"));
    let image = database.primary_index_image("t").unwrap();
    let before = database.committed_wal().unwrap();
    let baseline = database.view().unwrap().export_primary_tree("t").unwrap();
    let mut missing = baseline.clone();
    missing.remove(&Key::Integer(1)).unwrap();
    let mut extra = baseline.clone();
    extra
        .insert(
            Key::Integer(99),
            RecordPointer {
                page_id: 1,
                slot_id: 0,
            },
        )
        .unwrap();
    let mut wrong = baseline;
    wrong
        .replace(
            &Key::Integer(1),
            RecordPointer {
                page_id: u64::MAX,
                slot_id: u16::MAX,
            },
        )
        .unwrap();
    let mut forged = vec![
        with_tree(&image, missing, database.last_transaction()),
        with_tree(&image, extra, database.last_transaction()),
        with_tree(&image, wrong, database.last_transaction()),
        with_tree(&image, BPlusTree::new_stable(), database.last_transaction()),
    ];
    for offset in [16, 48] {
        let mut changed = image.clone();
        changed[offset] ^= 1;
        seal(&mut changed);
        forged.push(changed);
    }
    let nested = IndexSnapshot::decode(&image[INDEX_IMAGE_HEADER..]).unwrap();
    forged.push(with_tree(
        &image,
        nested.tree,
        database.last_transaction() + 1,
    ));
    for bad in forged {
        assert!(inspect_primary_index_image(&bad).is_ok());
        assert!(database.load_primary_index_image("t", &bad).is_err());
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(database.primary_index_image("t").unwrap(), image);
    }
}

#[test]
fn every_cut_byte_mutation_and_repaired_reserved_length_or_revision_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let database = initialized(&dir.path().join("db"));
    let image = database.primary_index_image("t").unwrap();
    for cut in 0..image.len() {
        assert!(
            inspect_primary_index_image(&image[..cut]).is_err(),
            "cut {cut}"
        );
    }
    for offset in 0..image.len() {
        let mut changed = image.clone();
        changed[offset] ^= 1;
        assert!(
            inspect_primary_index_image(&changed).is_err(),
            "byte {offset}"
        );
    }
    for offset in [10, 12, 40, 120] {
        let mut changed = image.clone();
        changed[offset] ^= 1;
        seal(&mut changed);
        assert!(inspect_primary_index_image(&changed).is_err());
    }
    let mut changed = image.clone();
    changed[80..88].copy_from_slice(&u64::MAX.to_le_bytes());
    let crc = crc32fast::hash(&changed[..124]);
    changed[124..128].copy_from_slice(&crc.to_le_bytes());
    assert!(inspect_primary_index_image(&changed).is_err());
    let mut trailing = image;
    trailing.push(0);
    assert!(inspect_primary_index_image(&trailing).is_err());
}

#[test]
fn checkpoint_reopen_compaction_and_both_wal_versions_keep_current_image_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path);
    let image = database.primary_index_image("t").unwrap();
    for compacted in [false, true] {
        if compacted {
            database.compact().unwrap();
        }
        database.checkpoint().unwrap();
        drop(database);
        database = Database::open(&path).unwrap();
        assert!(database.verify_primary_index_image("t", &image).is_ok());
        database.load_primary_index_image("t", &image).unwrap();
        assert_eq!(
            database
                .view()
                .unwrap()
                .scan_integer_range("t", Some(1), Some(3), 32)
                .unwrap(),
            vec![
                vec![Value::Integer(1), Value::Integer(7)],
                vec![Value::Integer(2), Value::Integer(14)]
            ]
        );
    }
}

#[test]
fn full_table_image_loads_every_key_and_creates_no_sidecar_or_wal_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = Database::create(&path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(schema("t", DataType::Integer))
        .unwrap();
    transaction.commit().unwrap();
    for start in (0..10_000).step_by(200) {
        let mut transaction = database.begin().unwrap();
        for key in start..start + 200 {
            transaction
                .insert("t", vec![Value::Integer(key), Value::Integer(key * 7)])
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    let directory_entries = || {
        let mut names = fs::read_dir(&path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let entries = directory_entries();
    let wal = database.committed_wal().unwrap();
    let pages = database.view().unwrap().page_fingerprint();
    let image = database.primary_index_image("t").unwrap();
    let report = inspect_primary_index_image(&image).unwrap();
    assert_eq!((report.entries, report.pages), (10_000, 768));
    assert_eq!(image.len(), INDEX_IMAGE_HEADER + 769 * 4096);
    assert!(image.len() <= MAX_INDEX_IMAGE_BYTES);
    let info = database.load_primary_index_image("t", &image).unwrap();
    assert_eq!((info.entries, info.pages), (report.entries, report.pages));
    for key in 0..10_000 {
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Integer(key))
                .unwrap(),
            Some(&vec![Value::Integer(key), Value::Integer(key * 7)])
        );
    }
    assert_eq!(
        database
            .view()
            .unwrap()
            .scan_integer_range("t", Some(9980), None, 100)
            .unwrap()
            .len(),
        20
    );
    assert_eq!(database.primary_index_image("t").unwrap(), image);
    assert_eq!(database.committed_wal().unwrap(), wal);
    assert_eq!(database.view().unwrap().page_fingerprint(), pages);
    assert_eq!(directory_entries(), entries);
    let oversized = vec![0; MAX_INDEX_IMAGE_BYTES + 1];
    assert!(inspect_primary_index_image(&oversized).is_err());
    assert!(inspect_primary_index_image(&vec![0; MAX_INDEX_IMAGE_BYTES]).is_err());
}

#[test]
fn boundary_text_and_empty_images_preserve_rows_excluded_from_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = Database::create(dir.path().join("db")).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(schema("t", DataType::Text))
        .unwrap();
    transaction
        .create_table(schema("empty", DataType::Integer))
        .unwrap();
    let keys = [
        String::new(),
        format!("{}a", "界".repeat(85)),
        format!("{}ab", "界".repeat(85)),
        "界".repeat(1024),
    ];
    for (id, key) in keys.iter().enumerate() {
        transaction
            .insert(
                "t",
                vec![Value::Text(key.clone()), Value::Integer(id as i64)],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    for name in ["t", "empty"] {
        let image = database.primary_index_image(name).unwrap();
        let report = database.verify_primary_index_image(name, &image).unwrap();
        let info = database.load_primary_index_image(name, &image).unwrap();
        assert_eq!(info.entries, report.entries);
        assert_eq!(info.excluded_long_keys, if name == "t" { 2 } else { 0 });
        assert_eq!(report.entries, if name == "t" { 2 } else { 0 });
        assert_eq!(report.pages, 1);
    }
    for (id, key) in keys.iter().enumerate() {
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t", &Key::Text(key.clone()))
                .unwrap(),
            Some(&vec![Value::Text(key.clone()), Value::Integer(id as i64)])
        );
    }
    let image = database.primary_index_image("t").unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .update(
            "t",
            &Key::Text(keys[3].clone()),
            vec![Value::Text(keys[3].clone()), Value::Integer(99)],
        )
        .unwrap();
    transaction.commit().unwrap();
    assert!(database.load_primary_index_image("t", &image).is_err());
    assert_eq!(
        database
            .view()
            .unwrap()
            .primary_index_info("t")
            .unwrap()
            .entries,
        2
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn arbitrary_image_input_is_bounded_and_deterministic(bytes in proptest::collection::vec(any::<u8>(),0..20000)) {
        prop_assert_eq!(inspect_primary_index_image(&bytes).ok(),inspect_primary_index_image(&bytes).ok());
    }
}
