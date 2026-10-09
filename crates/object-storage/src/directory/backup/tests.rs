use super::*;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn directory(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}
fn fixture() -> (tempfile::TempDir, PathBuf, ProjectDirectory) {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    directory(&source);
    let mut owner = ProjectDirectory::initialize(&source, PROJECT).unwrap();
    owner.put(OBJECT, b"synthetic-private\0\xff").unwrap();
    (temp, source, owner)
}

#[test]
fn publishes_exact_canonical_private_archive_and_keeps_complete_source_unchanged() {
    let (temp, source, owner) = fixture();
    let image = encode_archive(&owner.capture().unwrap()).unwrap();
    let expected = owner.inventory().unwrap();
    let marker = fs::read(source.join(SCOPE_FILE)).unwrap();
    let object = fs::read(source.join(object_name(OBJECT))).unwrap();
    let path = temp.path().join("copy.object-archive");
    let report = owner.backup_to(&path).unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(report.payload_bytes, 19);
    assert_eq!(report.digest, *expected.digest());
    assert_eq!(fs::read(&path).unwrap(), image);
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    assert_eq!(crate::inspect_archive_file(&path, PROJECT).unwrap(), report);
    assert!(owner.backup_to(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), image);
    assert_eq!(fs::read(source.join(SCOPE_FILE)).unwrap(), marker);
    assert_eq!(fs::read(source.join(object_name(OBJECT))).unwrap(), object);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    assert_eq!(owner.inventory().unwrap(), expected);
}

#[test]
fn refuses_source_parent_aliases_bad_names_and_symlinked_final_parent_before_capture() {
    let (temp, source, owner) = fixture();
    let alias = temp.path().join("alias");
    symlink(&source, &alias).unwrap();
    for path in [
        source.join("copy"),
        source.join("../source/copy"),
        alias.join("copy"),
        alias.join("../source/copy"),
        PathBuf::from("/"),
        PathBuf::from("."),
        PathBuf::from(".."),
        temp.path().join("missing/copy"),
        temp.path().join("nul\0name"),
    ] {
        let mut reached_capture = false;
        assert!(
            owner
                .backup_with(&path, || reached_capture = true, || {})
                .is_err()
        );
        assert!(!reached_capture);
    }
    assert_eq!(fs::read_dir(&source).unwrap().count(), 2);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn preserves_existing_files_directories_symlinks_and_hardlinks_without_overwrite() {
    for kind in 0..4 {
        let (temp, _, owner) = fixture();
        let path = temp.path().join("copy");
        let foreign = temp.path().join("foreign");
        fs::write(&foreign, b"synthetic-private-existing").unwrap();
        match kind {
            0 => fs::write(&path, b"synthetic-private-existing").unwrap(),
            1 => directory(&path),
            2 => symlink(&foreign, &path).unwrap(),
            _ => fs::hard_link(&foreign, &path).unwrap(),
        }
        let before = fs::symlink_metadata(&path).unwrap();
        assert!(owner.backup_to(&path).is_err());
        let after = fs::symlink_metadata(&path).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(fs::read(&foreign).unwrap(), b"synthetic-private-existing");
        if kind != 1 {
            assert_eq!(fs::read(&path).unwrap(), b"synthetic-private-existing");
        }
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 3);
    }
}

#[test]
fn refuses_changed_destination_parent_before_selection_and_preserves_foreign_directory() {
    let (temp, _, owner) = fixture();
    let parent = temp.path().join("destination");
    let moved = temp.path().join("moved");
    directory(&parent);
    let path = parent.join("copy");
    assert!(
        owner
            .backup_with(
                &path,
                || {
                    fs::rename(&parent, &moved).unwrap();
                    directory(&parent);
                    fs::write(parent.join("foreign"), b"synthetic-private").unwrap();
                },
                || panic!("must not select")
            )
            .is_err()
    );
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
    assert!(!path.exists());
    assert_eq!(
        fs::read(parent.join("foreign")).unwrap(),
        b"synthetic-private"
    );
}

#[test]
fn destination_parent_move_after_selection_reports_unknown_and_preserves_original_archive() {
    let (temp, _, owner) = fixture();
    let parent = temp.path().join("destination");
    let moved = temp.path().join("moved");
    directory(&parent);
    let path = parent.join("copy");
    let expected = encode_archive(&owner.capture().unwrap()).unwrap();
    let result = owner.backup_with(
        &path,
        || {},
        || {
            fs::rename(&parent, &moved).unwrap();
            directory(&parent);
            fs::write(&path, b"synthetic-private-foreign").unwrap();
        },
    );
    assert!(matches!(result, Err(Error::PublicationUnknown)));
    assert_eq!(fs::read(moved.join("copy")).unwrap(), expected);
    assert_eq!(fs::read(&path).unwrap(), b"synthetic-private-foreign");
    assert!(crate::inspect_archive_file(moved.join("copy"), PROJECT).is_ok());
    assert!(owner.backup_to(&path).is_err());
}

#[test]
fn source_namespace_move_uses_retained_directory_without_touching_replacement() {
    let (temp, source, owner) = fixture();
    let moved = temp.path().join("moved-source");
    let expected = encode_archive(&owner.capture().unwrap()).unwrap();
    let path = temp.path().join("copy");
    let report = owner
        .backup_with(
            &path,
            || {
                fs::rename(&source, &moved).unwrap();
                directory(&source);
                fs::write(source.join("foreign"), b"synthetic-private-foreign").unwrap();
            },
            || {},
        )
        .unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 2);
    assert_eq!(fs::read_dir(&source).unwrap().count(), 1);
}

#[test]
fn delayed_source_payload_scope_or_inventory_change_refuses_before_publication() {
    for mutation in 0..4 {
        let (temp, source, owner) = fixture();
        let path = temp.path().join("copy");
        assert!(
            owner
                .backup_with(
                    &path,
                    || {
                        match mutation {
                            0 => fs::write(
                                source.join(object_name(OBJECT)),
                                encode(PROJECT, OBJECT, b"changed").unwrap(),
                            )
                            .unwrap(),
                            1 => fs::write(
                                source.join(SCOPE_FILE),
                                encode(ProjectId::from_bytes([9; 16]), SCOPE_OBJECT, &[]).unwrap(),
                            )
                            .unwrap(),
                            2 => fs::write(source.join("unknown"), b"synthetic-private").unwrap(),
                            _ => {
                                fs::remove_file(source.join(object_name(OBJECT))).unwrap();
                            }
                        }
                    },
                    || panic!("must not select")
                )
                .is_err()
        );
        assert!(!path.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}

#[test]
fn final_archive_replacement_with_identical_bytes_still_requires_explicit_inspection() {
    let (temp, _, owner) = fixture();
    let path = temp.path().join("copy");
    let moved = temp.path().join("original");
    let result = owner.backup_with(
        &path,
        || {},
        || {
            fs::rename(&path, &moved).unwrap();
            fs::copy(&moved, &path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        },
    );
    assert!(matches!(result, Err(Error::PublicationUnknown)));
    assert_ne!(
        fs::metadata(&path).unwrap().ino(),
        fs::metadata(&moved).unwrap().ino()
    );
    assert_eq!(
        crate::inspect_archive_file(&path, PROJECT).unwrap(),
        crate::inspect_archive_file(&moved, PROJECT).unwrap()
    );
}

#[test]
fn final_corruption_alias_mode_or_source_marker_change_is_unknown_without_cleanup() {
    for mutation in 0..6 {
        let (temp, source, owner) = fixture();
        let path = temp.path().join("copy");
        let saved = temp.path().join("saved");
        let expected = encode_archive(&owner.capture().unwrap()).unwrap();
        let result = owner.backup_with(
            &path,
            || {},
            || match mutation {
                0 => fs::write(&path, b"damaged").unwrap(),
                1 => {
                    fs::rename(&path, &saved).unwrap();
                    symlink(&saved, &path).unwrap();
                }
                2 => fs::hard_link(&path, &saved).unwrap(),
                3 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
                4 => fs::write(source.join(SCOPE_FILE), b"damaged").unwrap(),
                _ => {
                    fs::rename(&path, &saved).unwrap();
                }
            },
        );
        assert!(matches!(result, Err(Error::PublicationUnknown)));
        if mutation != 0 {
            let preserved = if saved.exists() { &saved } else { &path };
            assert_eq!(fs::read(preserved).unwrap(), expected);
        } else {
            assert_eq!(fs::read(&path).unwrap(), b"damaged");
        }
    }
}

#[test]
fn canonical_empty_and_maximum_complete_archives_are_durably_published() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    directory(&source);
    let mut owner = ProjectDirectory::initialize(&source, PROJECT).unwrap();
    let empty = temp.path().join("empty");
    assert_eq!(owner.backup_to(&empty).unwrap().objects, 0);
    assert_eq!(fs::metadata(empty).unwrap().len(), 128);
    let payload = vec![0x59; MAX_PAYLOAD_BYTES];
    for key in 0..128 {
        owner
            .put(
                ObjectId::from_bytes([key; 16]),
                if key < 8 { &payload } else { &[] },
            )
            .unwrap();
    }
    let path = temp.path().join("maximum");
    let report = owner.backup_to(&path).unwrap();
    assert_eq!(report.objects, 128);
    assert_eq!(report.payload_bytes, crate::MAX_INVENTORY_BYTES);
    assert_eq!(fs::metadata(&path).unwrap().len(), MAX_ARCHIVE_BYTES as u64);
    assert_eq!(crate::inspect_archive_file(path, PROJECT).unwrap(), report);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_publications_match_independent_binary_model_and_refuse_retry(
        values in prop::collection::btree_map(0u8..16,prop::collection::vec(any::<u8>(),0..257),0..9)
    ) {
        let temp=tempfile::tempdir().unwrap();let source=temp.path().join("source");directory(&source);
        let mut owner=ProjectDirectory::initialize(&source,PROJECT).unwrap();
        for (&key,value) in &values {owner.put(ObjectId::from_bytes([key;16]),value).unwrap();}
        let path=temp.path().join("copy");let report=owner.backup_to(&path).unwrap();
        let bytes=fs::read(&path).unwrap();let view=crate::verify_archive(&bytes,PROJECT).unwrap();
        prop_assert_eq!(report.objects,values.len());prop_assert_eq!(view.objects().len(),values.len());
        prop_assert_eq!(report.payload_bytes,values.values().map(|value|value.len() as u64).sum::<u64>());
        for (object,(&key,value)) in view.objects().iter().zip(&values) {prop_assert_eq!(object.object(),ObjectId::from_bytes([key;16]));prop_assert_eq!(object.payload(),value);}
        prop_assert!(owner.backup_to(&path).is_err());prop_assert_eq!(fs::read(&path).unwrap(),bytes);
        prop_assert_eq!(fs::read_dir(temp.path()).unwrap().count(),2);
    }
}

#[test]
fn competing_complete_backups_select_exactly_one_whole_archive_and_preserve_all_sources() {
    let temp = tempfile::tempdir().unwrap();
    let mut owners = Vec::new();
    let mut images = Vec::new();
    for key in 0..8 {
        let source = temp.path().join(format!("source-{key}"));
        directory(&source);
        let mut owner = ProjectDirectory::initialize(&source, PROJECT).unwrap();
        owner.put(OBJECT, &vec![key; 16384 + key as usize]).unwrap();
        images.push(encode_archive(&owner.capture().unwrap()).unwrap());
        owners.push(owner);
    }
    let path = temp.path().join("selected");
    let barrier = std::sync::Barrier::new(8);
    let results = std::thread::scope(|scope| {
        let workers = owners
            .iter()
            .map(|owner| {
                let barrier = &barrier;
                let path = &path;
                scope.spawn(move || {
                    barrier.wait();
                    owner.backup_to(path)
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let selected = fs::read(&path).unwrap();
    let winner = results.iter().position(|result| result.is_ok()).unwrap();
    assert_eq!(selected, images[winner]);
    for (owner, expected) in owners.iter().zip(&images) {
        assert_eq!(
            encode_archive(&owner.capture().unwrap()).unwrap(),
            *expected
        );
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 9);
    assert_eq!(
        crate::inspect_archive_file(&path, PROJECT).unwrap(),
        *results[winner].as_ref().unwrap()
    );
}
