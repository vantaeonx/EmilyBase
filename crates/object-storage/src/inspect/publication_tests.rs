use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn selected_standalone_identity_must_survive_identical_final_name_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("selected");
    let saved = temp.path().join("original");
    let project = ProjectId::from_bytes([1; 16]);
    let object = ObjectId::from_bytes([2; 16]);
    let result = publish_file_with(&path, project, object, b"synthetic-private", || {
        fs::rename(&path, &saved).unwrap();
        fs::copy(&saved, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_ne!(
            fs::metadata(&path).unwrap().ino(),
            fs::metadata(&saved).unwrap().ino()
        );
    });
    assert!(matches!(result, Err(Error::PublicationUnknown)));
    assert_eq!(fs::read(&path).unwrap(), fs::read(&saved).unwrap());
}

#[test]
fn selected_standalone_mutations_are_unknown_without_cleanup_or_implicit_retry() {
    use std::os::unix::fs::symlink;
    let project = ProjectId::from_bytes([1; 16]);
    let object = ObjectId::from_bytes([2; 16]);
    let image = crate::encode(project, object, b"synthetic-private").unwrap();
    for mutation in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("selected");
        let saved = temp.path().join("original");
        let result =
            publish_file_with(
                &path,
                project,
                object,
                b"synthetic-private",
                || match mutation {
                    0 => {
                        fs::remove_file(&path).unwrap();
                    }
                    1 => {
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                    }
                    2 => {
                        fs::rename(&path, &saved).unwrap();
                        symlink(&saved, &path).unwrap();
                    }
                    3 => {
                        fs::hard_link(&path, &saved).unwrap();
                    }
                    4 => {
                        fs::write(&path, b"synthetic-corrupt").unwrap();
                    }
                    _ => {
                        fs::rename(&path, &saved).unwrap();
                        fs::create_dir(&path).unwrap();
                    }
                },
            );
        assert!(
            matches!(result, Err(Error::PublicationUnknown)),
            "mutation={mutation}"
        );
        match mutation {
            0 => assert!(!path.exists()),
            1 => {
                assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o644);
                assert_eq!(fs::read(&path).unwrap(), image);
            }
            2 => {
                assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
                assert_eq!(fs::read(&saved).unwrap(), image);
            }
            3 => {
                assert_eq!(fs::metadata(&path).unwrap().nlink(), 2);
                assert_eq!(fs::read(&saved).unwrap(), image);
            }
            4 => assert_eq!(fs::read(&path).unwrap(), b"synthetic-corrupt"),
            _ => {
                assert!(path.is_dir());
                assert_eq!(fs::read(&saved).unwrap(), image);
            }
        }
    }
}
