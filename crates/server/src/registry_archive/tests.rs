use super::*;
use crate::{ProjectStore, inspect_registry_backup, restore_registry_backup};
use emilybase_catalog::Value;
use proptest::prelude::*;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};

fn seed(root: &std::path::Path) -> (ProjectStore, Vec<(String, String)>) {
    let mut store = ProjectStore::open(root).unwrap();
    let mut credentials = Vec::new();
    for value in [7, 11] {
        let created = store.create("../../synthetic same name").unwrap();
        store
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY,v INT); INSERT INTO t VALUES(1,$1)",
                &[Value::Integer(value)],
            )
            .unwrap();
        credentials.push((created.project.id, created.api_key));
    }
    let rotated = store.rotate(&credentials[0].0).unwrap();
    credentials[0].1 = rotated.api_key;
    emilybase_transactions::Database::open(root.join(&credentials[1].0).join("data"))
        .unwrap()
        .compact()
        .unwrap();
    (store, credentials)
}

#[test]
fn registry_archive_roundtrip_preserves_scopes_epochs_ids_and_both_wal_versions() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let (mut store, credentials) = seed(&source);
    let image = store.backup_image().unwrap();
    let decoded = decode(&image).unwrap();
    assert_eq!(decoded.entries.len(), 2);
    assert!(decoded.entries[0].metadata.id < decoded.entries[1].metadata.id);
    let versions: BTreeSet<_> = decoded
        .report
        .projects
        .iter()
        .map(|p| p.wal_version)
        .collect();
    assert_eq!(versions, BTreeSet::from([1, 2]));
    let report_json = serde_json::to_string(&decoded.report).unwrap();
    for (_, key) in &credentials {
        assert!(!report_json.contains(key));
        assert!(
            !image
                .windows(key.len())
                .any(|window| window == key.as_bytes())
        );
    }
    let archive = temp.path().join("private.backup");
    let report = store.backup(&archive).unwrap();
    assert_eq!(inspect_registry_backup(&archive).unwrap(), report);
    let target = temp.path().join("restored");
    assert_eq!(restore_registry_backup(&archive, &target).unwrap(), report);
    let mut restored = ProjectStore::open(&target).unwrap();
    assert_eq!(restored.backup_image().unwrap(), image);
    for (i, (id, key)) in credentials.iter().enumerate() {
        let result = restored
            .authorize(id, key)
            .unwrap()
            .execute("SELECT * FROM t", &[])
            .unwrap();
        assert_eq!(result.transaction, 2);
        assert_eq!(result.results[0].rows[0][1], Value::Integer([7, 11][i]));
        assert!(restored.authorize(id, &credentials[1 - i].1).is_err());
        restored
            .authorize(id, key)
            .unwrap()
            .execute("INSERT INTO t VALUES(2,19)", &[])
            .unwrap();
        assert_eq!(store.authorize(id, key).unwrap().status().unwrap().rows, 1);
        assert_eq!(
            restored.authorize(id, key).unwrap().status().unwrap().rows,
            2
        );
    }
    let permissions = std::fs::metadata(&archive).unwrap().permissions().mode();
    assert_eq!(permissions & 0o777, 0o600);
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn empty_registry_is_canonical_and_restorable() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let mut store = ProjectStore::open(temp.path().join("empty")).unwrap();
    let image = store.backup_image().unwrap();
    assert_eq!(image.len(), HEADER);
    assert_eq!(&image[..16], b"EMILYREG\x01\0\x80\0\0\0\0\0");
    assert_eq!(&image[16..24], &[0; 8]);
    assert_eq!(&image[24..56], Sha256::digest([]).as_slice());
    assert!(image[56..124].iter().all(|b| *b == 0));
    assert_eq!(u32_at(&image, 124), 0x0ba888ef);
    let archive = temp.path().join("empty.backup");
    store.backup(&archive).unwrap();
    let target = temp.path().join("copy");
    restore_registry_backup(&archive, &target).unwrap();
    assert!(
        ProjectStore::open(target)
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn snapshot_refuses_outstanding_capabilities_and_every_external_database_owner() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (mut store, credentials) = seed(&temp.path().join("source"));
    let request = store
        .authorize(&credentials[0].0, &credentials[0].1)
        .unwrap();
    assert!(matches!(store.backup_image(), Err(Error::Busy)));
    drop(request);
    let owned = emilybase_transactions::Database::open(
        temp.path()
            .join("source")
            .join(&credentials[1].0)
            .join("data"),
    )
    .unwrap();
    assert!(store.backup_image().is_err());
    drop(owned);
    assert!(store.backup_image().is_ok());
}

#[test]
fn capture_rejects_replaced_root_changed_metadata_and_unsafe_archive_destination() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let (mut store, credentials) = seed(&root);
    assert!(matches!(
        store.backup(root.join("archive.backup")),
        Err(Error::Path)
    ));
    assert!(!root.join("archive.backup").exists());
    let path = root.join(&credentials[0].0).join("project.json");
    let prior = std::fs::read(&path).unwrap();
    let mut changed = metadata::read(&path, &credentials[0].0).unwrap();
    changed.epoch += 1;
    std::fs::write(&path, metadata::encoded(&changed).unwrap()).unwrap();
    assert!(matches!(store.backup_image(), Err(Error::Metadata)));
    std::fs::write(&path, prior).unwrap();
    std::fs::rename(&root, temp.path().join("moved")).unwrap();
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    assert!(matches!(store.backup_image(), Err(Error::Path)));
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn no_clobber_and_private_regular_single_link_archive_boundaries() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (mut store, _) = seed(&temp.path().join("source"));
    let archive = temp.path().join("archive.backup");
    store.backup(&archive).unwrap();
    let before = std::fs::read(&archive).unwrap();
    assert!(store.backup(&archive).is_err());
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    let protected = temp.path().join("protected");
    std::fs::write(&protected, b"preserve").unwrap();
    let link = temp.path().join("link");
    symlink(&protected, &link).unwrap();
    for target in [&protected, &link, &temp.path().join("source")] {
        assert!(restore_registry_backup(&archive, target).is_err());
    }
    assert_eq!(std::fs::read(&protected).unwrap(), b"preserve");
    assert!(inspect_registry_backup(&link).is_err());
    let alias = temp.path().join("alias");
    std::fs::hard_link(&archive, &alias).unwrap();
    assert!(matches!(
        inspect_registry_backup(&archive),
        Err(Error::Path)
    ));
    std::fs::remove_file(alias).unwrap();
    std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        inspect_registry_backup(&archive),
        Err(Error::Path)
    ));
}

fn image() -> Vec<u8> {
    let temp = tempfile::tempdir().unwrap();
    let (mut store, _) = seed(&temp.path().join("source"));
    store.backup_image().unwrap()
}

#[test]
fn every_truncation_and_each_single_byte_damage_fail_closed() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let bytes = image();
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end]).is_err());
    }
    for at in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[at] ^= 0x80;
        assert!(decode(&damaged).is_err());
    }
}

#[test]
fn repaired_outer_checksums_cannot_hide_semantic_entry_violations() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let bytes = image();
    for at in [HEADER, HEADER + 36, HEADER + 40] {
        let mut damaged = bytes.clone();
        damaged[at] = 0xff;
        finish(&mut damaged, 2).unwrap();
        assert!(decode(&damaged).is_err());
    }
    let meta = u32_at(&bytes[HEADER..], 32) as usize;
    let database_at = HEADER + ENTRY_HEADER + meta;
    let mut damaged = bytes.clone();
    damaged[database_at + 80] = 1;
    finish(&mut damaged, 2).unwrap();
    assert!(decode(&damaged).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    finish(&mut trailing, 2).unwrap();
    assert!(decode(&trailing).is_err());
    let mut zero_entries = bytes.clone();
    finish(&mut zero_entries, 0).unwrap();
    assert!(decode(&zero_entries).is_err());
}

#[test]
fn duplicate_database_identity_and_unordered_project_entries_refuse() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let bytes = image();
    let archive = decode(&bytes).unwrap();
    let mut duplicate = vec![0; HEADER];
    for entry in &archive.entries {
        append(&mut duplicate, &entry.metadata, archive.entries[0].database).unwrap();
    }
    finish(&mut duplicate, 2).unwrap();
    assert!(matches!(
        decode(&duplicate),
        Err(Error::RegistryFormat("duplicate database identity"))
    ));
    let mut unordered = vec![0; HEADER];
    for entry in archive.entries.iter().rev() {
        append(&mut unordered, &entry.metadata, entry.database).unwrap();
    }
    finish(&mut unordered, 2).unwrap();
    assert!(decode(&unordered).is_err());
}

#[test]
fn header_versions_reserved_lengths_and_metadata_canonicality_are_strict() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let bytes = image();
    for at in [8, 10, 12, 16, 56, 123] {
        let mut damaged = bytes.clone();
        damaged[at] ^= 0x80;
        let crc = crc32fast::hash(&damaged[..124]);
        damaged[124..128].copy_from_slice(&crc.to_le_bytes());
        assert!(decode(&damaged).is_err());
    }
    let archive = decode(&bytes).unwrap();
    let mut malformed = vec![0; HEADER];
    let entry = &archive.entries[0];
    let canonical = metadata::encoded(&entry.metadata).unwrap();
    let mut record = [0; ENTRY_HEADER];
    record[..32].copy_from_slice(entry.metadata.id.as_bytes());
    record[32..36].copy_from_slice(&((canonical.len() + 1) as u32).to_le_bytes());
    record[40..48].copy_from_slice(&(entry.database.len() as u64).to_le_bytes());
    malformed.extend_from_slice(&record);
    malformed.push(b' ');
    malformed.extend_from_slice(&canonical);
    malformed.extend_from_slice(entry.database);
    finish(&mut malformed, 1).unwrap();
    assert!(matches!(
        decode(&malformed),
        Err(Error::RegistryFormat("noncanonical metadata"))
    ));
}

#[test]
fn oversized_sparse_files_and_forged_entry_lengths_refuse_before_restore() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let oversized = temp.path().join("oversized.backup");
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&oversized)
        .unwrap()
        .set_len(MAX_REGISTRY_BACKUP_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(
        inspect_registry_backup(&oversized),
        Err(Error::Limit)
    ));
    let target = temp.path().join("unpublished");
    assert!(restore_registry_backup(&oversized, &target).is_err());
    assert!(!target.exists());
    let mut bytes = image();
    bytes[HEADER + 40..HEADER + 48].copy_from_slice(&u64::MAX.to_le_bytes());
    finish(&mut bytes, 2).unwrap();
    assert!(decode(&bytes).is_err());
}

#[test]
fn corrupt_source_or_committed_metadata_never_publishes_an_archive() {
    let _serial = crate::durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let (mut store, credentials) = seed(&root);
    let archive = temp.path().join("unpublished.backup");
    let project = root.join(&credentials[0].0);
    let metadata = project.join("project.json");
    let prior = std::fs::read(&metadata).unwrap();
    std::fs::write(&metadata, b"invalid").unwrap();
    assert!(store.backup(&archive).is_err());
    assert!(!archive.exists());
    std::fs::write(metadata, prior).unwrap();
    let wal = project.join("data/redo.wal");
    let mut damaged = std::fs::read(&wal).unwrap();
    damaged[0] ^= 0xff;
    std::fs::write(wal, damaged).unwrap();
    assert!(store.backup(&archive).is_err());
    assert!(!archive.exists());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_input_and_repaired_envelopes_are_bounded(bytes in proptest::collection::vec(any::<u8>(), 0..16384)) {
        let _ = inspect_registry_backup_bytes(&bytes);
        if bytes.len() >= HEADER {
            let mut repaired = bytes;
            finish(&mut repaired, 2).unwrap();
            let _ = inspect_registry_backup_bytes(&repaired);
        }
    }
}
