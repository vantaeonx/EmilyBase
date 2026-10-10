use super::*;
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_transactions::Database;
use std::path::Path;

fn original(path: &Path) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut tx = database.begin().unwrap();
    tx.create_table(Schema {
        name: "synthetic".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    })
    .unwrap();
    tx.insert("synthetic", vec![Value::Integer(1)]).unwrap();
    tx.commit().unwrap();
    database
}
fn add(path: &Path) {
    let mut database = Database::open(path).unwrap();
    let mut tx = database.begin().unwrap();
    tx.insert("synthetic", vec![Value::Integer(2)]).unwrap();
    tx.commit().unwrap();
}
#[derive(Debug, thiserror::Error)]
#[error("synthetic preparation refusal")]
struct Refusal;

#[test]
fn descriptor_restore_binds_original_moved_parent_and_refuses_nonleaf_or_replacement() {
    use std::fs::{self, File};
    use std::os::unix::ffi::OsStrExt;
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut source = original(&temp.path().join("source"));
        if compacted {
            source.compact().unwrap();
        }
        let wal = source.committed_wal().unwrap();
        let bytes = encode(&wal).unwrap();
        let expected = inspect_bytes(&bytes).unwrap();
        let parent = temp.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let owner = File::open(&parent).unwrap();
        let moved = temp.path().join("moved");
        fs::rename(&parent, &moved).unwrap();
        fs::create_dir(&parent).unwrap();
        for name in [
            b"".as_slice(),
            b".",
            b"..",
            b"../escape",
            b"a/b",
            b"nul\0leaf",
        ] {
            assert!(restore_bytes_at(&bytes, &owner, std::ffi::OsStr::from_bytes(name)).is_err());
            assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
        }
        assert_eq!(
            restore_bytes_at(&bytes, &owner, "restored").unwrap(),
            expected
        );
        let mut restored = Database::open(moved.join("restored")).unwrap();
        assert_eq!(restored.committed_wal().unwrap(), wal);
        drop(restored);
        assert!(restore_bytes_at(&bytes, &owner, "restored").is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        assert_eq!(
            Database::open(moved.join("restored"))
                .unwrap()
                .view()
                .unwrap()
                .row_count(),
            1
        );
    }
}

#[test]
fn preparation_commits_privately_and_returns_the_installed_report_on_both_wals() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let mut database = original(&source);
        if compacted {
            database.compact().unwrap();
        }
        let archive = dir.path().join("synthetic.backup");
        let before = create(&mut database, &archive).unwrap();
        let bytes = std::fs::read(&archive).unwrap();
        let wal = database.committed_wal().unwrap();
        let target = dir.path().join("installed");
        let report = restore_prepared(&archive, &target, |path| {
            assert!(!target.exists());
            add(path);
            assert!(!target.exists());
            Ok::<(), Refusal>(())
        })
        .unwrap();
        assert_eq!(report.database_id, before.database_id);
        assert_eq!(report.wal_version, before.wal_version);
        assert_eq!(report.last_transaction, before.last_transaction + 1);
        assert_eq!(report.rows, 2);
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        assert_eq!(database.committed_wal().unwrap(), wal);
        let mut installed = Database::open(&target).unwrap();
        assert!(
            installed
                .view()
                .unwrap()
                .get("synthetic", &Key::Integer(2))
                .unwrap()
                .is_some()
        );
        let rearchive = dir.path().join("installed.backup");
        assert_eq!(create(&mut installed, rearchive).unwrap(), report);
        let noop = dir.path().join("unchanged");
        assert_eq!(restore(&archive, noop).unwrap(), before);
    }
}

#[test]
fn preparation_refusal_after_commit_never_publishes_and_keeps_source_exact() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut database = original(&dir.path().join("source"));
    let archive = dir.path().join("synthetic.backup");
    create(&mut database, &archive).unwrap();
    let before = std::fs::read(&archive).unwrap();
    let target = dir.path().join("installed");
    let result = restore_prepared(&archive, &target, |path| {
        add(path);
        Err::<(), _>(Refusal)
    });
    assert!(matches!(
        result,
        Err(PreparedRestoreError::Preparation(Refusal))
    ));
    assert!(!target.exists());
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emilybase-backup-")
    }));
    restore_prepared(&archive, &target, |path| {
        add(path);
        Ok::<(), Refusal>(())
    })
    .unwrap();
    assert_eq!(
        Database::open(target).unwrap().view().unwrap().row_count(),
        2
    );
}

#[test]
fn malformed_archive_fails_before_calling_application_preparation() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("synthetic.backup");
    let mut database = original(&dir.path().join("source"));
    create(&mut database, &archive).unwrap();
    let good = std::fs::read(&archive).unwrap();
    for bad in [
        vec![],
        vec![0; HEADER_SIZE],
        {
            let mut b = good.clone();
            b.push(0);
            b
        },
        {
            let mut b = good.clone();
            b[HEADER_SIZE] ^= 1;
            b
        },
    ] {
        std::fs::write(&archive, bad).unwrap();
        let called = std::cell::Cell::new(false);
        let target = dir.path().join("installed");
        assert!(matches!(
            restore_prepared(&archive, &target, |_| {
                called.set(true);
                Ok::<(), Refusal>(())
            }),
            Err(PreparedRestoreError::Backup(_))
        ));
        assert!(!called.get());
        assert!(!target.exists());
    }
}

#[test]
fn changed_database_identity_and_corrupt_prepared_journal_are_replayed_before_publish() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    for changed_identity in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut database = original(&dir.path().join("source"));
        let mut foreign = original(&dir.path().join("foreign"));
        let archive = dir.path().join("synthetic.backup");
        create(&mut database, &archive).unwrap();
        let mut altered = if changed_identity {
            foreign.committed_wal().unwrap()
        } else {
            database.committed_wal().unwrap()
        };
        if !changed_identity {
            altered[0] ^= 1;
        }
        let target = dir.path().join("installed");
        assert!(matches!(
            restore_prepared(&archive, &target, |path| {
                std::fs::write(path.join("redo.wal"), altered).unwrap();
                Ok::<(), Refusal>(())
            }),
            Err(PreparedRestoreError::Backup(_))
        ));
        assert!(!target.exists());
    }
}

#[test]
fn application_must_release_its_database_owner_before_final_replay() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut database = original(&dir.path().join("source"));
    let archive = dir.path().join("synthetic.backup");
    create(&mut database, &archive).unwrap();
    let mut held = None;
    let target = dir.path().join("installed");
    assert!(matches!(
        restore_prepared(&archive, &target, |path| {
            held = Some(Database::open(path).unwrap());
            Ok::<(), Refusal>(())
        }),
        Err(PreparedRestoreError::Backup(_))
    ));
    assert!(!target.exists());
    drop(held);
}

#[test]
fn parent_or_staging_substitution_during_preparation_cannot_publish_or_delete_foreign_entries() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    for replace_parent in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("outputs");
        let moved = dir.path().join("moved");
        std::fs::create_dir(&parent).unwrap();
        let mut database = original(&dir.path().join("source"));
        let archive = dir.path().join("synthetic.backup");
        create(&mut database, &archive).unwrap();
        let target = parent.join("installed");
        let mut foreign = None;
        let result = restore_prepared(&archive, &target, |path| {
            add(path);
            let name = std::fs::read_dir(&parent)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .find(|n| n.to_string_lossy().starts_with(".emilybase-backup-"))
                .unwrap();
            if replace_parent {
                std::fs::rename(&parent, &moved).unwrap();
                std::fs::create_dir(&parent).unwrap();
            } else {
                std::fs::rename(parent.join(&name), &moved).unwrap();
            }
            let selected = parent.join(name);
            std::fs::create_dir(&selected).unwrap();
            std::fs::write(selected.join("marker"), b"synthetic-foreign").unwrap();
            foreign = Some(selected);
            Ok::<(), Refusal>(())
        });
        assert!(matches!(
            result,
            Err(PreparedRestoreError::Backup(Error::PathChanged))
        ));
        assert!(!target.exists());
        assert_eq!(
            std::fs::read(foreign.unwrap().join("marker")).unwrap(),
            b"synthetic-foreign"
        );
        if !replace_parent {
            assert_eq!(
                Database::open(moved).unwrap().view().unwrap().row_count(),
                2
            );
        }
    }
}

#[test]
fn prepared_restore_never_replaces_a_destination_directory_or_symlink() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut database = original(&dir.path().join("source"));
    let archive = dir.path().join("synthetic.backup");
    create(&mut database, &archive).unwrap();
    let target = dir.path().join("installed");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("marker"), b"synthetic-existing").unwrap();
    assert!(
        restore_prepared(&archive, &target, |path| {
            add(path);
            Ok::<(), Refusal>(())
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read(target.join("marker")).unwrap(),
        b"synthetic-existing"
    );
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert!(restore_prepared(&archive, alias, |_| Ok::<(), Refusal>(())).is_err());
    assert_eq!(
        std::fs::read(target.join("marker")).unwrap(),
        b"synthetic-existing"
    );
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn prepared_commits_and_rollbacks_follow_independent_row_model(events in prop::collection::vec((0..8_i64, any::<bool>()), 0..20)) {
        let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut source = original(&dir.path().join("source"));
        let archive = dir.path().join("synthetic.backup");
        create(&mut source, &archive).unwrap();
        let before = std::fs::read(&archive).unwrap();
        let target = dir.path().join("installed");
        let mut model = std::collections::BTreeSet::from([1]);
        let report = restore_prepared(&archive, &target, |path| {
            let mut database = Database::open(path).unwrap();
            for (id, commit) in events {
                let existed = model.contains(&id);
                let mut tx = database.begin().unwrap();
                if existed {tx.delete("synthetic", &Key::Integer(id)).unwrap();}
                else {tx.insert("synthetic", vec![Value::Integer(id)]).unwrap();}
                if commit {tx.commit().unwrap();if existed {model.remove(&id);} else {model.insert(id);}}
                else {drop(tx);}
            }
            Ok::<(), Refusal>(())
        }).unwrap();
        prop_assert_eq!(report.rows, model.len());
        prop_assert_eq!(std::fs::read(archive).unwrap(), before);
        let installed = Database::open(target).unwrap();
        let actual:std::collections::BTreeSet<_> = installed.view().unwrap().scan("synthetic", 20).unwrap().into_iter().map(|r| match r[0] {Value::Integer(i)=>i,_=>panic!("synthetic integer expected")}).collect();
        prop_assert_eq!(actual, model);
    }
}

#[test]
fn substituted_parent_after_prepared_publication_reports_unknown_and_preserves_the_prepared_state()
{
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("outputs");
        let moved = dir.path().join("moved");
        std::fs::create_dir(&parent).unwrap();
        let mut database = original(&dir.path().join("source"));
        if compacted {
            database.compact().unwrap();
        }
        let archive = dir.path().join("synthetic.backup");
        let before = create(&mut database, &archive).unwrap();
        let target = parent.join("installed");
        let result = crate::restore::restore_prepared_with(
            &archive,
            &target,
            |path| {
                add(path);
                Ok::<(), Refusal>(())
            },
            || {},
            || {
                std::fs::rename(&parent, &moved).unwrap();
                std::fs::create_dir(&parent).unwrap();
                std::fs::create_dir(&target).unwrap();
                std::fs::write(target.join("marker"), b"synthetic-foreign").unwrap();
            },
        );
        assert!(matches!(
            result,
            Err(PreparedRestoreError::Backup(Error::PublicationUnknown(_)))
        ));
        assert_eq!(
            std::fs::read(target.join("marker")).unwrap(),
            b"synthetic-foreign"
        );
        let installed = Database::open(moved.join("installed")).unwrap();
        assert_eq!(installed.last_transaction(), before.last_transaction + 1);
        assert_eq!(installed.view().unwrap().row_count(), 2);
        assert_eq!(inspect(archive).unwrap(), before);
    }
}

#[test]
fn byte_restore_and_preparation_preserve_input_and_install_the_prepared_report() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut source = original(&dir.path().join("source"));
        if compacted {
            source.compact().unwrap();
        }
        let bytes = encode(&source.committed_wal().unwrap()).unwrap();
        let before = bytes.clone();
        let expected = inspect_bytes(&bytes).unwrap();
        let target = dir.path().join("installed");
        let report = restore_prepared_bytes(&bytes, &target, |path| {
            assert!(!target.exists());
            add(path);
            Ok::<(), Refusal>(())
        })
        .unwrap();
        assert_eq!(report.last_transaction, expected.last_transaction + 1);
        assert_eq!(report.rows, 2);
        assert_eq!(report.database_id, expected.database_id);
        assert_eq!(bytes, before);
        assert_eq!(source.committed_wal().unwrap(), before[HEADER_SIZE..]);
        let mut installed = Database::open(&target).unwrap();
        assert_eq!(
            inspect_bytes(&encode(&installed.committed_wal().unwrap()).unwrap()).unwrap(),
            report
        );
        drop(installed);
        let plain = dir.path().join("plain");
        assert_eq!(restore_bytes(&bytes, &plain).unwrap(), expected);
        drop(bytes);
        assert_eq!(
            Database::open(plain).unwrap().view().unwrap().row_count(),
            1
        );
        // No input archive file was created anywhere in this operation.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}

#[test]
fn invalid_byte_images_never_reach_preparation_or_create_a_target() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut source = original(&dir.path().join("source"));
    let good = encode(&source.committed_wal().unwrap()).unwrap();
    for bytes in [
        vec![],
        vec![0; 128],
        {
            let mut bad = good.clone();
            bad.push(0);
            bad
        },
        {
            let mut bad = good.clone();
            bad[HEADER_SIZE] ^= 1;
            bad
        },
    ] {
        let called = std::cell::Cell::new(false);
        let target = dir.path().join("installed");
        assert!(matches!(
            restore_prepared_bytes(&bytes, &target, |_| {
                called.set(true);
                Ok::<(), Refusal>(())
            }),
            Err(PreparedRestoreError::Backup(_))
        ));
        assert!(!called.get());
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn byte_preparation_refusal_and_existing_targets_keep_the_source_and_foreign_state() {
    let _io = publication_tests::PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut source = original(&dir.path().join("source"));
    let bytes = encode(&source.committed_wal().unwrap()).unwrap();
    let before = bytes.clone();
    let target = dir.path().join("installed");
    assert!(matches!(
        restore_prepared_bytes(&bytes, &target, |path| {
            add(path);
            Err::<(), _>(Refusal)
        }),
        Err(PreparedRestoreError::Preparation(Refusal))
    ));
    assert!(!target.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("marker"), b"synthetic-foreign").unwrap();
    assert!(restore_bytes(&bytes, &target).is_err());
    let link = dir.path().join("alias");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(restore_bytes(&bytes, link).is_err());
    assert_eq!(
        std::fs::read(target.join("marker")).unwrap(),
        b"synthetic-foreign"
    );
    assert_eq!(bytes, before);
    assert_eq!(source.committed_wal().unwrap(), before[HEADER_SIZE..]);
}
