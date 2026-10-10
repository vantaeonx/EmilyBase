use super::*;
use crate::{Error, ProjectId, encode};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FOREIGN: ProjectId = ProjectId::from_bytes([3; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn directory(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}
fn replace_identical(path: &Path, saved: &Path) {
    fs::rename(path, saved).unwrap();
    fs::copy(saved, path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_ne!(
        fs::metadata(path).unwrap().ino(),
        fs::metadata(saved).unwrap().ino()
    );
}

#[test]
fn reports_match_owned_reads_and_independent_hash_at_empty_scratch_and_maximum_sizes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    for (index, size) in [0, 8191, 8192, 8193, MAX_PAYLOAD_BYTES]
        .into_iter()
        .enumerate()
    {
        let object = ObjectId::from_bytes([index as u8 + 2; 16]);
        let payload: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let written = owner.put(object, &payload).unwrap();
        let report = owner.inspect(object).unwrap();
        assert_eq!(report, written);
        assert_eq!(&report, owner.get(object).unwrap().report());
        assert_eq!(report.payload_bytes, size);
        assert_eq!(report.sha256, Sha256::digest(&payload).as_slice());
        let before = fs::metadata(path.join(object_name(object))).unwrap();
        fs::set_permissions(
            path.join(object_name(object)),
            fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        assert_eq!(owner.inspect(object).unwrap(), report);
        let after = fs::metadata(path.join(object_name(object))).unwrap();
        assert_eq!(
            (before.len(), before.mtime(), before.mtime_nsec()),
            (after.len(), after.mtime(), after.mtime_nsec())
        );
    }
}

#[test]
fn complete_late_payload_corruption_and_foreign_envelopes_refuse_without_repairs() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    owner.put(OBJECT, &vec![0xa5; 32769]).unwrap();
    let target = path.join(object_name(OBJECT));
    let mut bytes = fs::read(&target).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&target, &bytes).unwrap();
    assert!(matches!(owner.inspect(OBJECT), Err(Error::PayloadChecksum)));
    assert_eq!(fs::read(&target).unwrap(), bytes);
    let foreign = encode(FOREIGN, OBJECT, b"synthetic-foreign").unwrap();
    fs::write(&target, &foreign).unwrap();
    assert!(matches!(owner.inspect(OBJECT), Err(Error::Scope)));
    assert_eq!(fs::read(&target).unwrap(), foreign);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
}

#[test]
fn post_verification_object_and_scope_changes_refuse_and_preserve_artifacts() {
    for mutation in 0..9 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        directory(&path);
        let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
        owner.put(OBJECT, b"synthetic-private").unwrap();
        let object = path.join(object_name(OBJECT));
        let scope = path.join(super::super::SCOPE_FILE);
        let saved = temp.path().join("saved");
        let alias = temp.path().join("alias");
        let result = owner.inspect_with(OBJECT, || match mutation {
            0 => replace_identical(&object, &saved),
            1 => {
                let mut file = File::options().write(true).open(&object).unwrap();
                file.seek(SeekFrom::End(-1)).unwrap();
                file.write_all(b"!").unwrap();
                file.sync_all().unwrap();
            }
            2 => fs::set_permissions(&object, fs::Permissions::from_mode(0o644)).unwrap(),
            3 => fs::hard_link(&object, &alias).unwrap(),
            4 => fs::remove_file(&object).unwrap(),
            5 => {
                fs::rename(&object, &saved).unwrap();
                symlink(&saved, &object).unwrap();
            }
            6 => replace_identical(&scope, &saved),
            7 => File::options()
                .write(true)
                .open(&scope)
                .unwrap()
                .set_len(0)
                .unwrap(),
            8 => fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap(),
            _ => unreachable!(),
        });
        assert!(result.is_err(), "mutation {mutation}");
        assert!(path.is_dir());
        if matches!(mutation, 0 | 5 | 6) {
            assert!(saved.is_file());
        }
        if mutation == 3 {
            assert!(alias.is_file());
        }
        if mutation == 4 {
            assert!(!object.exists());
        } else {
            assert!(object.exists());
        }
    }
}

#[test]
fn renamed_owned_directory_does_not_read_or_modify_the_replacement_namespace() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("objects");
    let moved = temp.path().join("moved");
    directory(&original);
    let mut owner = ProjectDirectory::initialize(&original, PROJECT).unwrap();
    let expected = owner.put(OBJECT, b"synthetic-owned").unwrap();
    fs::rename(&original, &moved).unwrap();
    directory(&original);
    let mut replacement = ProjectDirectory::initialize(&original, FOREIGN).unwrap();
    replacement.put(OBJECT, b"synthetic-replacement").unwrap();
    let foreign_before = fs::read(original.join(object_name(OBJECT))).unwrap();
    assert_eq!(owner.inspect(OBJECT).unwrap(), expected);
    assert_eq!(
        fs::read(original.join(object_name(OBJECT))).unwrap(),
        foreign_before
    );
    assert!(matches!(
        owner.inspect(ObjectId::from_bytes([0x77; 16])),
        Err(Error::Io(_))
    ));
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 2);
}

#[test]
fn selected_inspection_does_not_claim_complete_inventory_or_admit_unsafe_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    let expected = owner.put(OBJECT, b"synthetic-private").unwrap();
    fs::write(path.join("unknown-stage"), b"synthetic-unmanaged").unwrap();
    assert_eq!(owner.inspect(OBJECT).unwrap(), expected);
    assert!(owner.inventory().is_err());
    let target = path.join(object_name(OBJECT));
    for mode in [0o644, 0o666] {
        fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
        assert!(matches!(owner.inspect(OBJECT), Err(Error::File)));
    }
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    File::options()
        .write(true)
        .open(&target)
        .unwrap()
        .set_len((HEADER_BYTES + MAX_PAYLOAD_BYTES + 1) as u64)
        .unwrap();
    assert!(matches!(owner.inspect(OBJECT), Err(Error::Limit)));
    assert!(path.join("unknown-stage").is_file());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_native_reports_match_exact_binary_images(payload in prop::collection::vec(any::<u8>(), 0..33000)) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        directory(&path);
        let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
        let expected = owner.put(OBJECT, &payload).unwrap();
        prop_assert_eq!(owner.inspect(OBJECT).unwrap(), expected);
        prop_assert_eq!(fs::read(path.join(object_name(OBJECT))).unwrap(), encode(PROJECT, OBJECT, &payload).unwrap());
    }
}
