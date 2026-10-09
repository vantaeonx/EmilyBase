use super::*;
use crate::{ObjectId, encode_archive};
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::path::PathBuf;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn image(values: &[(ObjectId, &[u8])]) -> Vec<u8> {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
    let mut owner = ProjectDirectory::initialize(&source, PROJECT).unwrap();
    for (id, payload) in values {
        owner.put(*id, payload).unwrap();
    }
    encode_archive(&owner.capture().unwrap()).unwrap()
}
fn archive(parent: &Path) -> PathBuf {
    let path = parent.join("source.object-archive");
    emilybase_storage::publish_private_file(
        &path,
        &image(&[(OBJECT, b"synthetic-private\0\xff")]),
        MAX_ARCHIVE_BYTES,
    )
    .unwrap();
    path
}
fn stage(parent: &Path) -> PathBuf {
    fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .as_encoded_bytes()
                .starts_with(b".emilybase-directory-")
        })
        .unwrap()
}

#[test]
fn restores_complete_bytes_same_scope_private_modes_and_independent_mutability() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(temp.path());
    let original = fs::read(&path).unwrap();
    let target = temp.path().join("restored");
    let report = restore_archive_file(&path, PROJECT, &target).unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(report.payload_bytes, 19);
    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o700);
    let mut owner = ProjectDirectory::open(&target, PROJECT).unwrap();
    assert_eq!(
        owner.get(OBJECT).unwrap().payload(),
        b"synthetic-private\0\xff"
    );
    assert_eq!(encode_archive(&owner.capture().unwrap()).unwrap(), original);
    let object = target.join(format!("{OBJECT}.object"));
    assert_eq!(fs::metadata(&object).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&object).unwrap().nlink(), 1);
    owner.put(ObjectId::from_bytes([4; 16]), b"new").unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(restore_archive_file(&path, PROJECT, &target).is_err());
    assert_eq!(owner.inventory().unwrap().entries().len(), 2);
}

#[test]
fn complete_malformed_or_foreign_archives_refuse_before_creating_any_stage() {
    let bytes = image(&[(OBJECT, b"synthetic-private")]);
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("restored");
    for end in 0..bytes.len() {
        assert!(restore_archive(&bytes[..end], PROJECT, &target).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    for i in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[i] ^= 1;
        assert!(restore_archive(&changed, PROJECT, &target).is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    assert!(restore_archive(&bytes, ProjectId::from_bytes([9; 16]), &target).is_err());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn existing_target_and_final_parent_alias_are_preserved() {
    for kind in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let path = archive(temp.path());
        let target = temp.path().join("restored");
        let foreign = temp.path().join("foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("data"), b"synthetic-foreign").unwrap();
        match kind {
            0 => fs::write(&target, b"synthetic-foreign").unwrap(),
            1 => fs::create_dir(&target).unwrap(),
            _ => symlink(&foreign, &target).unwrap(),
        }
        let before = fs::symlink_metadata(&target).unwrap();
        assert!(restore_archive_file(&path, PROJECT, &target).is_err());
        assert_eq!(fs::symlink_metadata(&target).unwrap().ino(), before.ino());
        assert_eq!(
            fs::read(foreign.join("data")).unwrap(),
            b"synthetic-foreign"
        );
        // Complete but unselected stages are preserved, never recursively swept.
        let retained = stage(temp.path());
        drop(ProjectDirectory::open(retained, PROJECT).unwrap());
    }
    let temp = tempfile::tempdir().unwrap();
    let path = archive(temp.path());
    let alias = temp.path().join("alias");
    symlink(temp.path(), &alias).unwrap();
    assert!(restore_archive_file(path, PROJECT, alias.join("restored")).is_err());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn corrupted_populated_stage_or_parent_substitution_refuses_and_preserves_partial_contents() {
    for mutation in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let path = archive(temp.path());
        let parent = temp.path().join("destination");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("restored");
        let moved = temp.path().join("moved");
        let result = restore_file_with(
            &path,
            PROJECT,
            &target,
            || {
                let partial = stage(&parent);
                match mutation {
                    0 => fs::write(partial.join("unknown"), b"synthetic-foreign").unwrap(),
                    1 => fs::write(partial.join(format!("{OBJECT}.object")), b"damaged").unwrap(),
                    _ => {
                        fs::rename(&parent, &moved).unwrap();
                        fs::create_dir(&parent).unwrap();
                        fs::write(parent.join("foreign"), b"synthetic-foreign").unwrap();
                    }
                }
            },
            || panic!("must not select"),
        );
        assert!(matches!(result, Err(Error::RestoreStage(_))));
        assert!(!target.exists());
        let preserved = if mutation == 2 {
            stage(&moved)
        } else {
            stage(&parent)
        };
        assert!(preserved.exists());
        if mutation != 1 {
            drop(ProjectDirectory::open(preserved, PROJECT).unwrap());
        }
        assert!(crate::inspect_archive_file(&path, PROJECT).is_ok());
    }
}

#[test]
fn changed_source_file_after_populating_stage_refuses_before_selection() {
    for mutation in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let path = archive(temp.path());
        let target = temp.path().join("restored");
        let saved = temp.path().join("saved");
        let result = restore_file_with(
            &path,
            PROJECT,
            &target,
            || match mutation {
                0 => fs::write(&path, b"damaged").unwrap(),
                1 => {
                    fs::rename(&path, &saved).unwrap();
                    fs::copy(&saved, &path).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                }
                _ => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            },
            || panic!("must not select"),
        );
        assert!(matches!(result, Err(Error::RestoreStage(_))));
        assert!(!target.exists());
        drop(ProjectDirectory::open(stage(temp.path()), PROJECT).unwrap());
    }
}

#[test]
fn postselection_parent_name_or_data_mutation_is_unknown_without_removing_selected_directory() {
    for mutation in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let path = archive(temp.path());
        let parent = temp.path().join("destination");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("restored");
        let moved = temp.path().join("moved");
        let result = restore_file_with(
            &path,
            PROJECT,
            &target,
            || {},
            || match mutation {
                0 => {
                    fs::rename(&parent, &moved).unwrap();
                    fs::create_dir(&parent).unwrap();
                }
                1 => {
                    fs::rename(&target, &moved).unwrap();
                    fs::create_dir(&target).unwrap();
                }
                2 => fs::write(target.join(format!("{OBJECT}.object")), b"damaged").unwrap(),
                _ => fs::write(target.join("unknown"), b"synthetic-foreign").unwrap(),
            },
        );
        assert!(matches!(result, Err(Error::PublicationUnknown)));
        let original = match mutation {
            0 => moved.join("restored"),
            1 => moved,
            _ => target,
        };
        assert!(original.join(".emilybase-objects").exists());
        assert!(crate::inspect_archive_file(&path, PROJECT).is_ok());
        if mutation < 2 {
            let owner = ProjectDirectory::open(original, PROJECT).unwrap();
            assert_eq!(
                owner.get(OBJECT).unwrap().payload(),
                b"synthetic-private\0\xff"
            );
        }
    }
}

#[test]
fn empty_and_exact_maximum_archives_restore_complete_private_directories() {
    let temp = tempfile::tempdir().unwrap();
    let empty = temp.path().join("empty");
    assert_eq!(
        restore_archive(&image(&[]), PROJECT, &empty)
            .unwrap()
            .objects,
        0
    );
    assert_eq!(fs::read_dir(empty).unwrap().count(), 1);
    let payload = vec![0x59; crate::MAX_PAYLOAD_BYTES];
    let values = (0..128)
        .map(|key| {
            (
                ObjectId::from_bytes([key; 16]),
                if key < 8 { payload.as_slice() } else { &[][..] },
            )
        })
        .collect::<Vec<_>>();
    let bytes = image(&values);
    assert_eq!(bytes.len(), MAX_ARCHIVE_BYTES);
    let target = temp.path().join("maximum");
    let path = temp.path().join("maximum.object-archive");
    emilybase_storage::publish_private_file(&path, &bytes, MAX_ARCHIVE_BYTES).unwrap();
    let report = restore_archive_file(&path, PROJECT, &target).unwrap();
    assert_eq!(report.objects, 128);
    assert_eq!(report.payload_bytes, crate::MAX_INVENTORY_BYTES);
    let owner = ProjectDirectory::open(target, PROJECT).unwrap();
    assert_eq!(encode_archive(&owner.capture().unwrap()).unwrap(), bytes);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_restores_match_independent_binary_model_and_exact_archive(
        values in prop::collection::btree_map(0u8..16,prop::collection::vec(any::<u8>(),0..257),0..9)
    ) {
        let parts=values.iter().map(|(&key,value)|(ObjectId::from_bytes([key;16]),value.as_slice())).collect::<Vec<_>>();let bytes=image(&parts);let temp=tempfile::tempdir().unwrap();let target=temp.path().join("restored");
        let report=restore_archive(&bytes,PROJECT,&target).unwrap();let owner=ProjectDirectory::open(&target,PROJECT).unwrap();prop_assert_eq!(report.objects,values.len());
        for (&key,value) in &values {let object=owner.get(ObjectId::from_bytes([key;16])).unwrap();prop_assert_eq!(object.payload(),value);}
        prop_assert_eq!(encode_archive(&owner.capture().unwrap()).unwrap(),bytes);
    }
}

#[test]
fn invalid_private_source_aliases_modes_and_oversize_refuse_before_any_stage() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(temp.path());
    let target = temp.path().join("restored");
    let alias = temp.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(restore_archive_file(&alias, PROJECT, &target).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    assert!(restore_archive_file(&path, PROJECT, &target).is_err());
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(restore_archive_file(&path, PROJECT, &target).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len((MAX_ARCHIVE_BYTES + 1) as u64)
        .unwrap();
    assert!(matches!(
        restore_archive_file(&path, PROJECT, &target),
        Err(Error::Limit)
    ));
    assert!(!target.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn competing_complete_restores_select_one_directory_and_preserve_unselected_private_stages() {
    let temp = tempfile::tempdir().unwrap();
    let path = archive(temp.path());
    let target = temp.path().join("restored");
    let barrier = std::sync::Barrier::new(4);
    let results = std::thread::scope(|scope| {
        let workers = (0..4)
            .map(|_| {
                let barrier = &barrier;
                let path = &path;
                let target = &target;
                scope.spawn(move || {
                    barrier.wait();
                    restore_archive_file(path, PROJECT, target)
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let original = fs::read(&path).unwrap();
    for entry in fs::read_dir(temp.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let owner = ProjectDirectory::open(path, PROJECT).unwrap();
            assert_eq!(encode_archive(&owner.capture().unwrap()).unwrap(), original);
        }
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 5);
}
