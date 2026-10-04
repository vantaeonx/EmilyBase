use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_index::{BPlusTree, IndexSnapshot, RecordPointer};
use emilybase_transactions::{
    Database, INDEX_IMAGE_HEADER, MAX_CACHE_WARMUP_BYTES, MAX_INDEX_IMAGE_BYTES,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;

fn initialized(path: &Path, count: usize) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    for id in 0..count {
        transaction
            .create_table(Schema {
                name: format!("t{id}"),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                }],
                primary_key: 0,
            })
            .unwrap();
    }
    transaction.commit().unwrap();
    database
}
fn active(path: &Path, id: u64) -> std::path::PathBuf {
    path.join(format!("primary-{id}.table-index"))
}
fn insert(database: &mut Database, table: &str, key: i64) {
    let mut transaction = database.begin().unwrap();
    transaction
        .insert(table, vec![Value::Integer(key)])
        .unwrap();
    transaction.commit().unwrap();
}

#[test]
fn startup_missing_loaded_and_stale_counts_do_not_write_any_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path, 3);
    insert(&mut database, "t0", 7);
    assert_eq!(database.primary_cache_startup().unwrap().loaded, 0);
    let wal = database.committed_wal().unwrap();
    drop(database);
    let database = Database::open(&path).unwrap();
    let report = database.primary_cache_startup().unwrap();
    assert_eq!(
        (
            report.loaded,
            report.missing,
            report.rejected,
            report.skipped,
            report.bytes_budgeted
        ),
        (0, 3, 0, 0, 0)
    );
    assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    database.save_primary_index_cache("t0").unwrap();
    let image = fs::read(active(&path, 1)).unwrap();
    drop(database);
    let mut database = Database::open(&path).unwrap();
    let startup = database.primary_cache_startup().unwrap();
    assert_eq!(
        (startup.loaded, startup.missing, startup.rejected),
        (1, 2, 0)
    );
    assert_eq!(startup.bytes_budgeted, image.len() + 1);
    assert_eq!(database.committed_wal().unwrap(), wal);
    assert_eq!(fs::read(active(&path, 1)).unwrap(), image);
    insert(&mut database, "t1", 9);
    let current = database.warm_primary_index_caches().unwrap();
    assert_eq!(
        (current.loaded, current.missing, current.rejected),
        (0, 2, 1)
    );
    assert_eq!(database.primary_cache_startup().unwrap(), startup);
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("t0", &Key::Integer(7))
            .unwrap(),
        Some(&vec![Value::Integer(7)])
    );
}

#[test]
fn foreign_stale_and_rehashed_wrong_projection_fall_back_independently() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path, 4);
    let other = initialized(&dir.path().join("other"), 4);
    for id in 0..4 {
        database
            .save_primary_index_cache(&format!("t{id}"))
            .unwrap();
    }
    insert(&mut database, "t0", 7);
    let foreign = other.primary_index_image("t0").unwrap();
    fs::write(active(&path, 1), foreign).unwrap();
    let mut wrong = database.primary_index_image("t2").unwrap()[..INDEX_IMAGE_HEADER].to_vec();
    let mut tree = BPlusTree::new_stable();
    tree.insert(
        Key::Integer(99),
        RecordPointer {
            page_id: 1,
            slot_id: 0,
        },
    )
    .unwrap();
    let payload = IndexSnapshot {
        revision: database.last_transaction(),
        tree,
    }
    .encode()
    .unwrap();
    wrong[80..88].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    wrong[88..120].copy_from_slice(&Sha256::digest(&payload));
    let crc = crc32fast::hash(&wrong[..124]);
    wrong[124..128].copy_from_slice(&crc.to_le_bytes());
    wrong.extend_from_slice(&payload);
    assert!(emilybase_transactions::inspect_primary_index_image(&wrong).is_ok());
    fs::write(active(&path, 3), &wrong).unwrap();
    database.save_primary_index_cache("t3").unwrap();
    let wal = database.committed_wal().unwrap();
    drop(database);
    let mut database = Database::open(&path).unwrap();
    let report = database.primary_cache_startup().unwrap();
    assert_eq!(
        (
            report.loaded,
            report.rejected,
            report.missing,
            report.skipped
        ),
        (1, 3, 0, 0)
    );
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("t0", &Key::Integer(7))
            .unwrap(),
        Some(&vec![Value::Integer(7)])
    );
    assert!(
        database
            .view()
            .unwrap()
            .get("t2", &Key::Integer(99))
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read(active(&path, 3)).unwrap(), wrong);
    assert_eq!(database.committed_wal().unwrap(), wal);
}

#[test]
fn oversized_candidate_set_is_bounded_and_later_small_valid_images_still_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let database = initialized(&path, 32);
    for id in 1..=30 {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(active(&path, id))
            .unwrap();
        file.set_len(MAX_INDEX_IMAGE_BYTES as u64).unwrap();
    }
    for id in 30..32 {
        database
            .save_primary_index_cache(&format!("t{id}"))
            .unwrap();
    }
    drop(database);
    let database = Database::open(&path).unwrap();
    let report = database.primary_cache_startup().unwrap();
    assert_eq!(
        (
            report.loaded,
            report.rejected,
            report.skipped,
            report.missing
        ),
        (2, 3, 27, 0)
    );
    assert_eq!(
        report.bytes_budgeted,
        3 * (MAX_INDEX_IMAGE_BYTES + 1) + 2 * (INDEX_IMAGE_HEADER + 8192 + 1)
    );
    assert!(report.bytes_budgeted <= MAX_CACHE_WARMUP_BYTES);
    assert_eq!(database.view().unwrap().row_count(), 0);
    assert_eq!(
        fs::metadata(active(&path, 1)).unwrap().len(),
        MAX_INDEX_IMAGE_BYTES as u64
    );
}

#[test]
fn all_128_tables_and_ten_thousand_rows_fit_the_real_startup_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path, 128);
    for start in (0..10_000).step_by(200) {
        let mut transaction = database.begin().unwrap();
        for key in start..start + 200 {
            transaction.insert("t0", vec![Value::Integer(key)]).unwrap();
        }
        transaction.commit().unwrap();
    }
    let mut budget = 0;
    for id in 0..128 {
        database
            .save_primary_index_cache(&format!("t{id}"))
            .unwrap();
        budget += fs::metadata(active(&path, id + 1)).unwrap().len() as usize + 1;
    }
    assert!(budget < MAX_CACHE_WARMUP_BYTES);
    let wal = database.committed_wal().unwrap();
    drop(database);
    let mut database = Database::open(&path).unwrap();
    let startup = database.primary_cache_startup().unwrap();
    assert_eq!(
        (
            startup.loaded,
            startup.missing,
            startup.rejected,
            startup.skipped
        ),
        (128, 0, 0, 0)
    );
    assert_eq!(startup.bytes_budgeted, budget);
    for key in 0..10_000 {
        assert_eq!(
            database
                .view()
                .unwrap()
                .get("t0", &Key::Integer(key))
                .unwrap(),
            Some(&vec![Value::Integer(key)])
        );
    }
    assert_eq!(database.committed_wal().unwrap(), wal);
    let mut transaction = database.begin().unwrap();
    transaction
        .update("t0", &Key::Integer(0), vec![Value::Integer(0)])
        .unwrap();
    transaction.commit().unwrap();
    let current = database.warm_primary_index_caches().unwrap();
    assert_eq!(
        (current.loaded, current.rejected, current.skipped),
        (0, 128, 0)
    );
    assert_eq!(current.bytes_budgeted, budget);
    assert_eq!(database.primary_cache_startup().unwrap(), startup);
}

#[test]
fn private_path_failures_are_optional_but_damaged_or_missing_wal_is_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let database = initialized(&path, 3);
    database.save_primary_index_cache("t0").unwrap();
    let valid = fs::read(active(&path, 1)).unwrap();
    fs::set_permissions(active(&path, 1), fs::Permissions::from_mode(0o644)).unwrap();
    symlink(active(&path, 1), active(&path, 2)).unwrap();
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        active(&path, 3),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    drop(database);
    let database = Database::open(&path).unwrap();
    let report = database.primary_cache_startup().unwrap();
    assert_eq!(
        (report.loaded, report.rejected, report.bytes_budgeted),
        (0, 3, 0)
    );
    assert_eq!(fs::read(active(&path, 1)).unwrap(), valid);
    drop(database);
    fs::set_permissions(active(&path, 1), fs::Permissions::from_mode(0o600)).unwrap();
    let wal_path = path.join("redo.wal");
    let mut wal = fs::read(&wal_path).unwrap();
    wal[0] ^= 1;
    fs::write(&wal_path, &wal).unwrap();
    assert!(Database::open(&path).is_err());
    assert_eq!(fs::read(active(&path, 1)).unwrap(), valid);
    fs::remove_file(wal_path).unwrap();
    assert!(Database::open(&path).is_err());
}

#[test]
fn public_root_keeps_wal_behavior_and_final_aliases_cannot_bypass_directory_admission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut database = initialized(&path, 2);
    insert(&mut database, "t0", 7);
    database.save_primary_index_cache("t0").unwrap();
    let wal = database.committed_wal().unwrap();
    drop(database);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let database = Database::open(&path).unwrap();
    let report = database.primary_cache_startup().unwrap();
    assert_eq!(
        (report.loaded, report.rejected, report.bytes_budgeted),
        (0, 2, 0)
    );
    assert_eq!(
        database
            .view()
            .unwrap()
            .get("t0", &Key::Integer(7))
            .unwrap(),
        Some(&vec![Value::Integer(7)])
    );
    drop(database);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = dir.path().join("alias");
    symlink(&path, &alias).unwrap();
    let cache = fs::read(active(&path, 1)).unwrap();
    assert!(Database::open(alias).is_err());
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
    assert_eq!(fs::read(active(&path, 1)).unwrap(), cache);
    let database = Database::open(&path).unwrap();
    assert_eq!(database.primary_cache_startup().unwrap().loaded, 1);
    assert_eq!(database.view().unwrap().row_count(), 1);
    assert_eq!(fs::read(path.join("redo.wal")).unwrap(), wal);
}
