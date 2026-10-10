use super::*;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn owner(parent: &Path) -> ProjectDirectory {
    let path = parent.join("objects");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    ProjectDirectory::initialize(path, PROJECT).unwrap()
}

#[test]
fn actual_selected_descriptor_and_expected_metadata_survive_later_work_under_original_owner() {
    let temp = tempfile::tempdir().unwrap();
    let mut directory = owner(temp.path());
    let path = temp.path().join("objects").join(object_name(OBJECT));
    let mut selected = directory
        .put_selected(OBJECT, b"synthetic-private\0\xff")
        .unwrap();
    assert_eq!(selected.project(), PROJECT);
    assert_eq!(selected.object(), OBJECT);
    assert_eq!(selected.report().payload_bytes, 19);
    assert_eq!(
        selected.file.metadata().unwrap().ino(),
        fs::metadata(&path).unwrap().ino()
    );
    assert!(matches!(
        ProjectDirectory::open(temp.path().join("objects"), PROJECT),
        Err(Error::Busy)
    ));
    fs::write(temp.path().join("unrelated"), b"synthetic external work").unwrap();
    for _ in 0..3 {
        selected.verify().unwrap();
    }
    assert!(!format!("{selected:?}").contains("synthetic-private"));
    drop(selected);
    assert_eq!(
        directory.get(OBJECT).unwrap().payload(),
        b"synthetic-private\0\xff"
    );
    drop(directory);
    ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
}

#[test]
fn bounded_selection_retains_original_complete_receipt_without_granting_a_later_inventory_lease() {
    let temp = tempfile::tempdir().unwrap();
    let mut directory = owner(temp.path());
    directory
        .put(ObjectId::from_bytes([3; 16]), b"older")
        .unwrap();
    let limits = WriteLimits::new(2, 14).unwrap();
    let mut selected = directory
        .put_bounded_selected(OBJECT, b"synthetic", limits)
        .unwrap();
    assert_eq!(selected.project(), PROJECT);
    assert_eq!(selected.object(), OBJECT);
    assert_eq!(selected.report().payload_bytes, 9);
    assert_eq!(selected.inventory().entries().len(), 2);
    assert_eq!(selected.inventory().payload_bytes(), 14);
    let expected = selected.inventory().clone();
    selected.verify().unwrap();
    fs::write(
        temp.path().join("objects/unknown"),
        b"foreign unmanaged entry",
    )
    .unwrap();
    selected.verify().unwrap();
    assert_eq!(selected.inventory(), &expected);
    assert!(matches!(
        selected.selected.owner.inventory(),
        Err(Error::Inventory)
    ));
    assert!(!format!("{selected:?}").contains("synthetic"));
    drop(selected);
    fs::remove_file(temp.path().join("objects/unknown")).unwrap();
    assert_eq!(directory.inventory().unwrap(), expected);
    assert!(matches!(
        directory.put_bounded_selected(ObjectId::from_bytes([4; 16]), b"", limits),
        Err(Error::Limit)
    ));
}

fn mutate(parent: &Path, shape: u8) {
    let directory = parent.join("objects");
    let path = directory.join(object_name(OBJECT));
    match shape {
        0 => fs::write(path, b"damaged").unwrap(),
        1 => fs::write(
            path,
            encode(ProjectId::from_bytes([9; 16]), OBJECT, b"synthetic").unwrap(),
        )
        .unwrap(),
        2 => fs::write(
            path,
            encode(PROJECT, ObjectId::from_bytes([9; 16]), b"synthetic").unwrap(),
        )
        .unwrap(),
        3 => fs::write(path, encode(PROJECT, OBJECT, b"different").unwrap()).unwrap(),
        4 => File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(HEADER_BYTES as u64)
            .unwrap(),
        5 => {
            use std::io::Write;
            File::options()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(b"x")
                .unwrap();
        }
        6 => fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap(),
        7 => fs::hard_link(path, parent.join("linked")).unwrap(),
        8 | 9 => {
            let saved = parent.join("saved");
            fs::rename(&path, &saved).unwrap();
            if shape == 8 {
                symlink(&saved, path).unwrap();
            } else {
                fs::copy(&saved, path).unwrap();
            }
        }
        10 => {
            let marker = directory.join(SCOPE_FILE);
            fs::rename(&marker, parent.join("old-marker")).unwrap();
            fs::write(&marker, encode(PROJECT, SCOPE_OBJECT, b"").unwrap()).unwrap();
        }
        11 => fs::write(directory.join(SCOPE_FILE), b"broken scope").unwrap(),
        _ => fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap(),
    }
}

#[test]
fn later_byte_scope_metadata_and_same_byte_inode_mutations_refuse_without_cleanup_or_adoption() {
    for shape in 0..13 {
        let temp = tempfile::tempdir().unwrap();
        let mut directory = owner(temp.path());
        let mut selected = directory.put_selected(OBJECT, b"synthetic").unwrap();
        let original = selected.file.metadata().unwrap().ino();
        mutate(temp.path(), shape);
        assert!(
            matches!(selected.verify(), Err(Error::PublicationUnknown)),
            "shape {shape}"
        );
        assert_eq!(selected.file.metadata().unwrap().ino(), original);
        assert!(
            temp.path()
                .join("objects")
                .join(object_name(OBJECT))
                .symlink_metadata()
                .is_ok()
        );
        if matches!(shape, 8 | 9) {
            assert_eq!(
                fs::read(temp.path().join("saved")).unwrap(),
                encode(PROJECT, OBJECT, b"synthetic").unwrap()
            );
        }
        if shape == 12 {
            fs::set_permissions(
                temp.path().join("objects"),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
    }
}

#[test]
fn mutations_after_complete_body_verification_are_rechecked_before_returning_success() {
    for shape in 0..13 {
        let temp = tempfile::tempdir().unwrap();
        let mut directory = owner(temp.path());
        let mut selected = directory.put_selected(OBJECT, b"synthetic").unwrap();
        let result = selected.verify_with(|| mutate(temp.path(), shape));
        assert!(
            matches!(result, Err(Error::PublicationUnknown)),
            "shape {shape}"
        );
        assert!(
            temp.path()
                .join("objects")
                .join(object_name(OBJECT))
                .symlink_metadata()
                .is_ok()
        );
        if shape == 12 {
            fs::set_permissions(
                temp.path().join("objects"),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
    }
}

#[test]
fn bounded_selection_keeps_its_own_inode_after_identical_replacement_and_current_report_match() {
    let temp = tempfile::tempdir().unwrap();
    let mut directory = owner(temp.path());
    let mut selected = directory
        .put_bounded_selected(OBJECT, b"synthetic", WriteLimits::new(1, 9).unwrap())
        .unwrap();
    let original = selected.selected.file.metadata().unwrap().ino();
    mutate(temp.path(), 9);
    assert_eq!(
        selected.selected.owner.inspect(OBJECT).unwrap(),
        *selected.report()
    );
    assert_ne!(
        fs::metadata(temp.path().join("objects").join(object_name(OBJECT)))
            .unwrap()
            .ino(),
        original
    );
    assert!(matches!(selected.verify(), Err(Error::PublicationUnknown)));
    assert_eq!(selected.selected.file.metadata().unwrap().ino(), original);
}

#[test]
fn renamed_directory_and_replacement_path_never_redirect_retained_selection() {
    let temp = tempfile::tempdir().unwrap();
    let mut directory = owner(temp.path());
    let mut selected = directory.put_selected(OBJECT, b"synthetic").unwrap();
    let path = temp.path().join("objects");
    let moved = temp.path().join("moved");
    fs::rename(&path, &moved).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let mut replacement = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    replacement.put(OBJECT, b"different").unwrap();
    selected.verify().unwrap();
    assert_eq!(selected.report().payload_bytes, 9);
    assert_eq!(
        fs::read(moved.join(object_name(OBJECT))).unwrap(),
        encode(PROJECT, OBJECT, b"synthetic").unwrap()
    );
    assert_eq!(replacement.get(OBJECT).unwrap().payload(), b"different");
}

#[test]
fn empty_and_maximum_selections_revalidate_exact_private_bytes_without_owned_readback_images() {
    for payload in [vec![], vec![0x91; MAX_PAYLOAD_BYTES]] {
        let temp = tempfile::tempdir().unwrap();
        let mut directory = owner(temp.path());
        let mut selected = directory
            .put_bounded_selected(
                OBJECT,
                &payload,
                WriteLimits::new(1, MAX_PAYLOAD_BYTES as u64).unwrap(),
            )
            .unwrap();
        selected.verify().unwrap();
        let path = temp.path().join("objects").join(object_name(OBJECT));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        selected.verify().unwrap();
        assert_eq!(selected.report().payload_bytes, payload.len());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_retained_selections_match_owned_model_across_later_reads(
        payload in prop::collection::vec(any::<u8>(),0..4096), bounded in any::<bool>(),
    ) {
        let temp=tempfile::tempdir().unwrap();let mut directory=owner(temp.path());
        if bounded {
            let mut selected=directory.put_bounded_selected(OBJECT,&payload,WriteLimits::new(1,4096).unwrap()).unwrap();
            selected.verify().unwrap();
            let data=selected.selected.owner.get(OBJECT).unwrap();
            prop_assert_eq!(data.report(),selected.report());
        } else {
            let mut selected=directory.put_selected(OBJECT,&payload).unwrap();selected.verify().unwrap();
            let data=selected.owner.get(OBJECT).unwrap();
            prop_assert_eq!(data.report(),selected.report());
        }
        let data=directory.get(OBJECT).unwrap();
        prop_assert_eq!(data.payload(),payload);
    }
}
