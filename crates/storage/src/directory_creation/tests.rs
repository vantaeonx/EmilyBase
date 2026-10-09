use super::*;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

fn child(stage: &StagedPrivateDirectory) {
    crate::publish_private_file_at(stage.directory(), "child", b"synthetic-private", 64).unwrap();
}

#[test]
fn private_directory_selection_retains_exact_handles_and_never_replaces_existing_names() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("selected");
    let stage = StagedPrivateDirectory::new(&path).unwrap();
    let inode = stage.directory().metadata().unwrap().ino();
    child(&stage);
    let published = stage.publish().unwrap();
    published.check().unwrap();
    assert_eq!(published.directory().metadata().unwrap().ino(), inode);
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    assert_eq!(fs::read(path.join("child")).unwrap(), b"synthetic-private");
    let stage = StagedPrivateDirectory::new(&path).unwrap();
    assert!(stage.publish().is_err());
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn abandonment_removes_only_unchanged_empty_stage_and_preserves_nonempty_contents() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("selected");
    drop(StagedPrivateDirectory::new(&path).unwrap());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    let stage = StagedPrivateDirectory::new(&path).unwrap();
    let name = stage.stage.clone();
    child(&stage);
    drop(stage);
    assert!(!path.exists());
    assert_eq!(
        fs::read(temp.path().join(name).join("child")).unwrap(),
        b"synthetic-private"
    );
}

#[test]
fn substituted_stage_is_not_selected_or_removed_and_detached_original_is_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("selected");
    let stage = StagedPrivateDirectory::new(&path).unwrap();
    child(&stage);
    let name = temp.path().join(&stage.stage);
    let moved = temp.path().join("moved");
    fs::rename(&name, &moved).unwrap();
    fs::create_dir(&name).unwrap();
    fs::write(name.join("foreign"), b"synthetic-foreign").unwrap();
    assert!(matches!(stage.publish(), Err(Error::PathChanged)));
    assert!(!path.exists());
    assert_eq!(fs::read(moved.join("child")).unwrap(), b"synthetic-private");
    assert_eq!(
        fs::read(name.join("foreign")).unwrap(),
        b"synthetic-foreign"
    );
}

#[test]
fn changed_parent_before_selection_cannot_redirect_or_sweep_either_directory() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("parent");
    let moved = temp.path().join("moved");
    fs::create_dir(&parent).unwrap();
    let path = parent.join("selected");
    let stage = StagedPrivateDirectory::new(&path).unwrap();
    let name = stage.stage.clone();
    child(&stage);
    fs::rename(&parent, &moved).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::write(parent.join("foreign"), b"synthetic-foreign").unwrap();
    assert!(matches!(stage.publish(), Err(Error::PathChanged)));
    assert!(!path.exists());
    assert!(!moved.join("selected").exists());
    assert_eq!(
        fs::read(moved.join(name).join("child")).unwrap(),
        b"synthetic-private"
    );
    assert_eq!(
        fs::read(parent.join("foreign")).unwrap(),
        b"synthetic-foreign"
    );
}

#[test]
fn postselection_parent_or_name_replacement_is_unknown_and_preserves_complete_original() {
    for mutation in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let path = parent.join("selected");
        let stage = StagedPrivateDirectory::new(&path).unwrap();
        child(&stage);
        let saved = temp.path().join("saved");
        let result = stage.publish_with(|| match mutation {
            0 => {
                fs::rename(&parent, &saved).unwrap();
                fs::create_dir(&parent).unwrap();
            }
            1 => {
                fs::rename(&path, &saved).unwrap();
                fs::create_dir(&path).unwrap();
            }
            _ => {
                fs::rename(&path, &saved).unwrap();
                symlink(&saved, &path).unwrap();
            }
        });
        assert!(matches!(result, Err(Error::PublicationUnknown(_))));
        let original = if mutation == 0 {
            saved.join("selected")
        } else {
            saved
        };
        assert_eq!(
            fs::read(original.join("child")).unwrap(),
            b"synthetic-private"
        );
    }
}

#[test]
fn invalid_paths_symlinked_parent_and_changed_stage_permissions_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let alias = temp.path().join("alias");
    symlink(temp.path(), &alias).unwrap();
    for path in [
        PathBuf::from("."),
        PathBuf::from(".."),
        PathBuf::from("/"),
        temp.path().join("nul\0name"),
        temp.path().join("missing/selected"),
        alias.join("selected"),
    ] {
        assert!(StagedPrivateDirectory::new(path).is_err());
    }
    let path = temp.path().join("selected");
    let stage = StagedPrivateDirectory::new(&path).unwrap();
    let name = temp.path().join(&stage.stage);
    fs::set_permissions(&name, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(stage.publish().is_err());
    assert!(!path.exists());
    assert!(!name.exists());
}

#[test]
fn original_sync_faults_distinguish_private_unselected_stage_from_selected_unknown_directory() {
    use crate::publication_tests::{CASES, FaultGuard};
    let _serial = CASES.lock().unwrap();
    for (phase, after, selected) in [
        ("directory_sync", false, false),
        ("directory_sync", true, false),
        ("parent_sync", false, true),
        ("parent_sync", true, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("selected");
        let stage = StagedPrivateDirectory::new(&path).unwrap();
        let name = stage.stage.clone();
        child(&stage);
        let _fault = FaultGuard::new(phase, after);
        let result = stage.publish();
        if selected {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            assert_eq!(fs::read(path.join("child")).unwrap(), b"synthetic-private");
            assert!(!temp.path().join(name).exists());
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
            assert!(!path.exists());
            assert_eq!(
                fs::read(temp.path().join(name).join("child")).unwrap(),
                b"synthetic-private"
            );
        }
    }
}
