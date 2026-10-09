use super::*;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
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
fn selected_object_identity_must_survive_identical_final_name_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    let target = path.join(object_name(OBJECT));
    let saved = temp.path().join("original");
    let result = owner.put_with(OBJECT, b"synthetic-private", || {
        replace_identical(&target, &saved)
    });
    assert!(matches!(result, Err(Error::PublicationUnknown)));
    assert_eq!(fs::read(&target).unwrap(), fs::read(&saved).unwrap());
}

#[test]
fn selected_scope_identity_must_survive_identical_final_name_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let target = path.join(SCOPE_FILE);
    let saved = temp.path().join("original");
    let result =
        ProjectDirectory::initialize_owned_with(open_directory(&path).unwrap(), PROJECT, || {
            replace_identical(&target, &saved)
        });
    assert!(matches!(result, Err(Error::PublicationUnknown)));
    assert_eq!(fs::read(&target).unwrap(), fs::read(&saved).unwrap());
    assert!(ProjectDirectory::open(&path, PROJECT).is_ok());
}

#[test]
fn selected_native_scope_and_object_mutations_are_unknown_and_never_cleaned_up() {
    use std::os::unix::fs::symlink;
    for scope in [false, true] {
        for mutation in 0..6 {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("objects");
            directory(&path);
            let target = path.join(if scope {
                SCOPE_FILE.to_owned()
            } else {
                object_name(OBJECT)
            });
            let saved = temp.path().join("original");
            let image = encode(
                PROJECT,
                if scope { SCOPE_OBJECT } else { OBJECT },
                if scope { b"" } else { b"synthetic-private" },
            )
            .unwrap();
            let mutate = || match mutation {
                0 => {
                    fs::remove_file(&target).unwrap();
                }
                1 => {
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
                }
                2 => {
                    fs::rename(&target, &saved).unwrap();
                    symlink(&saved, &target).unwrap();
                }
                3 => {
                    fs::hard_link(&target, &saved).unwrap();
                }
                4 => {
                    fs::write(&target, b"synthetic-corrupt").unwrap();
                }
                _ => {
                    fs::rename(&target, &saved).unwrap();
                    directory(&target);
                }
            };
            let unknown = if scope {
                matches!(
                    ProjectDirectory::initialize_owned_with(
                        open_directory(&path).unwrap(),
                        PROJECT,
                        mutate
                    ),
                    Err(Error::PublicationUnknown)
                )
            } else {
                let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
                matches!(
                    owner.put_with(OBJECT, b"synthetic-private", mutate),
                    Err(Error::PublicationUnknown)
                )
            };
            assert!(unknown, "scope={scope}, mutation={mutation}");
            match mutation {
                0 => assert!(!target.exists()),
                1 => {
                    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o644);
                    assert_eq!(fs::read(&target).unwrap(), image);
                }
                2 => {
                    assert!(fs::symlink_metadata(&target).unwrap().is_symlink());
                    assert_eq!(fs::read(&saved).unwrap(), image);
                }
                3 => {
                    assert_eq!(fs::metadata(&target).unwrap().nlink(), 2);
                    assert_eq!(fs::read(&saved).unwrap(), image);
                }
                4 => assert_eq!(fs::read(&target).unwrap(), b"synthetic-corrupt"),
                _ => {
                    assert!(target.is_dir());
                    assert_eq!(fs::read(&saved).unwrap(), image);
                }
            }
        }
    }
}
