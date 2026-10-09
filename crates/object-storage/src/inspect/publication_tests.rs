use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn streamed_native_report_refuses_observed_changes_after_complete_hash_without_cleanup() {
    let project = ProjectId::from_bytes([1; 16]);
    let object = ObjectId::from_bytes([2; 16]);
    for mutation in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("selected");
        let alias = temp.path().join("alias");
        publish_file(&path, project, object, b"synthetic-private").unwrap();
        let mut file = open_private(&path).unwrap();
        let result = read_open_report_with(&mut file, project, object, || match mutation {
            0 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            1 => fs::hard_link(&path, &alias).unwrap(),
            2 => fs::remove_file(&path).unwrap(),
            _ => {
                use std::io::Write;
                File::options()
                    .append(true)
                    .open(&path)
                    .unwrap()
                    .write_all(b"x")
                    .unwrap();
            }
        });
        assert!(result.is_err(), "mutation={mutation}");
        match mutation {
            0 => assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o644),
            1 => {
                assert_eq!(fs::metadata(&path).unwrap().nlink(), 2);
                assert_eq!(fs::read(&path).unwrap(), fs::read(&alias).unwrap());
            }
            2 => assert!(!path.exists()),
            _ => assert_eq!(
                fs::metadata(&path).unwrap().len(),
                (HEADER_BYTES + 18) as u64
            ),
        }
    }
}

#[test]
fn streamed_native_inspection_rejects_identical_name_replacement_after_hash() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("selected");
    let saved = temp.path().join("original");
    let project = ProjectId::from_bytes([1; 16]);
    let object = ObjectId::from_bytes([2; 16]);
    publish_file(&path, project, object, b"synthetic-private").unwrap();
    let result = inspect_file_with(&path, project, object, || {
        fs::rename(&path, &saved).unwrap();
        fs::copy(&saved, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    });
    assert!(matches!(result, Err(Error::File)));
    assert_ne!(
        fs::metadata(&path).unwrap().ino(),
        fs::metadata(&saved).unwrap().ino()
    );
    assert_eq!(
        inspect_file(&path, project, object).unwrap(),
        inspect_file(&saved, project, object).unwrap()
    );
}

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
