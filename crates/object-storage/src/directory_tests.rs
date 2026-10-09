use super::*;
use proptest::prelude::*;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FOREIGN: ProjectId = ProjectId::from_bytes([3; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
const MARKER: &str = ".emilybase-objects";
fn directory(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}
fn private(path: &Path, image: &[u8]) {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(image).unwrap();
    file.sync_all().unwrap();
}
fn name(object: ObjectId) -> String {
    format!("{object}.object")
}

#[test]
fn private_directory_initializes_once_retains_lock_and_reopens_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    assert_eq!(owner.project(), PROJECT);
    assert!(matches!(
        ProjectDirectory::open(&path, PROJECT),
        Err(Error::Busy)
    ));
    assert!(matches!(
        ProjectDirectory::initialize(&path, PROJECT),
        Err(Error::Busy)
    ));
    let report = owner.put(OBJECT, b"synthetic-private-payload").unwrap();
    let data = owner.get(OBJECT).unwrap();
    assert_eq!(data.report(), &report);
    assert_eq!(data.payload(), b"synthetic-private-payload");
    assert_eq!(data.project(), PROJECT);
    assert_eq!(data.object(), OBJECT);
    assert!(!format!("{data:?}").contains("synthetic-private"));
    let before = fs::read(path.join(name(OBJECT))).unwrap();
    assert!(owner.put(OBJECT, b"replacement").is_err());
    assert_eq!(fs::read(path.join(name(OBJECT))).unwrap(), before);
    let marker = fs::read(path.join(MARKER)).unwrap();
    assert_eq!(
        marker,
        encode(PROJECT, ObjectId::from_bytes([0; 16]), &[]).unwrap()
    );
    drop(owner);
    assert!(ProjectDirectory::open(&path, FOREIGN).is_err());
    assert!(ProjectDirectory::initialize(&path, FOREIGN).is_err());
    assert_eq!(fs::read(path.join(MARKER)).unwrap(), marker);
    let reopened = ProjectDirectory::open(&path, PROJECT).unwrap();
    assert_eq!(reopened.get(OBJECT).unwrap().payload(), data.payload());
    assert_eq!(fs::read(path.join(name(OBJECT))).unwrap(), before);
    assert_eq!(fs::read_dir(path).unwrap().count(), 2);
}

#[test]
fn independent_project_directories_preserve_equal_object_names_without_cross_scope_reads() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    directory(&a);
    directory(&b);
    let mut first = ProjectDirectory::initialize(&a, PROJECT).unwrap();
    let mut second = ProjectDirectory::initialize(&b, FOREIGN).unwrap();
    first.put(OBJECT, b"synthetic-project-a").unwrap();
    second.put(OBJECT, b"synthetic-project-b").unwrap();
    assert_eq!(first.get(OBJECT).unwrap().payload(), b"synthetic-project-a");
    assert_eq!(
        second.get(OBJECT).unwrap().payload(),
        b"synthetic-project-b"
    );
    // A valid same-ID foreign image is still refused, without selecting a repair.
    let foreign = fs::read(b.join(name(OBJECT))).unwrap();
    fs::write(a.join(name(OBJECT)), &foreign).unwrap();
    assert!(matches!(first.get(OBJECT), Err(Error::Scope)));
    assert!(first.put(OBJECT, b"repair").is_err());
    assert_eq!(fs::read(a.join(name(OBJECT))).unwrap(), foreign);
    assert_eq!(
        second.get(OBJECT).unwrap().payload(),
        b"synthetic-project-b"
    );
}

#[test]
fn renamed_owned_directory_keeps_exact_inode_and_never_redirects_into_replacement_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("selected");
    let moved = temp.path().join("moved");
    directory(&path);
    let mut original = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    original.put(OBJECT, b"synthetic-original").unwrap();
    fs::rename(&path, &moved).unwrap();
    directory(&path);
    let mut foreign = ProjectDirectory::initialize(&path, FOREIGN).unwrap();
    foreign
        .put(OBJECT, b"synthetic-replacement-directory")
        .unwrap();
    let next = ObjectId::from_bytes([4; 16]);
    original.put(next, b"synthetic-after-move").unwrap();
    assert!(!path.join(name(next)).exists());
    assert!(moved.join(name(next)).is_file());
    assert_eq!(
        original.get(next).unwrap().payload(),
        b"synthetic-after-move"
    );
    assert_eq!(
        foreign.get(OBJECT).unwrap().payload(),
        b"synthetic-replacement-directory"
    );
    assert!(matches!(
        ProjectDirectory::open(&moved, PROJECT),
        Err(Error::Busy)
    ));
    drop(original);
    assert_eq!(
        ProjectDirectory::open(&moved, PROJECT)
            .unwrap()
            .get(OBJECT)
            .unwrap()
            .payload(),
        b"synthetic-original"
    );
}

#[test]
fn marker_replacement_foreign_content_permissions_and_aliases_refuse_current_operations() {
    for mutation in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        directory(&path);
        let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
        owner.put(OBJECT, b"synthetic-existing").unwrap();
        let marker = path.join(MARKER);
        let original = fs::read(&marker).unwrap();
        match mutation {
            0 => {
                // Identical bytes in a different inode cannot replace held scope.
                fs::rename(&marker, temp.path().join("detached")).unwrap();
                private(&marker, &original);
            }
            1 => fs::write(
                &marker,
                encode(FOREIGN, ObjectId::from_bytes([0; 16]), &[]).unwrap(),
            )
            .unwrap(),
            2 => fs::set_permissions(&marker, fs::Permissions::from_mode(0o644)).unwrap(),
            3 => fs::hard_link(&marker, temp.path().join("alias")).unwrap(),
            4 => {
                fs::rename(&marker, temp.path().join("detached")).unwrap();
                symlink(temp.path().join("detached"), &marker).unwrap();
            }
            _ => fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap(),
        }
        let before = fs::read(path.join(name(OBJECT))).unwrap();
        assert!(owner.get(OBJECT).is_err(), "mutation {mutation}");
        assert!(
            owner.put(ObjectId::from_bytes([4; 16]), b"new").is_err(),
            "mutation {mutation}"
        );
        assert!(!path.join(name(ObjectId::from_bytes([4; 16]))).exists());
        assert_eq!(fs::read(path.join(name(OBJECT))).unwrap(), before);
    }
}

#[test]
fn directory_open_never_creates_repairs_or_accepts_public_missing_alias_or_wrong_marker() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    assert!(ProjectDirectory::initialize(&path, PROJECT).is_err());
    assert!(!path.exists());
    directory(&path);
    assert!(ProjectDirectory::open(&path, PROJECT).is_err());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(ProjectDirectory::initialize(&path, PROJECT).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = temp.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(ProjectDirectory::initialize(&alias, PROJECT).is_err());
    assert!(ProjectDirectory::open(&alias, PROJECT).is_err());
    let marker = path.join(MARKER);
    for bytes in [
        b"damaged".to_vec(),
        encode(PROJECT, ObjectId::from_bytes([0; 16]), b"nonempty").unwrap(),
    ] {
        fs::write(&marker, &bytes).unwrap();
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(ProjectDirectory::open(&path, PROJECT).is_err());
        assert!(ProjectDirectory::initialize(&path, PROJECT).is_err());
        assert_eq!(fs::read(&marker).unwrap(), bytes);
    }
}

#[test]
fn object_reads_refuse_aliases_corruption_public_modes_and_oversize_without_repair() {
    for mutation in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        directory(&path);
        let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
        owner.put(OBJECT, b"synthetic-original").unwrap();
        let target = path.join(name(OBJECT));
        match mutation {
            0 => fs::hard_link(&target, temp.path().join("alias")).unwrap(),
            1 => {
                fs::rename(&target, temp.path().join("detached")).unwrap();
                symlink(temp.path().join("detached"), &target).unwrap();
            }
            2 => fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap(),
            3 => fs::write(&target, b"damaged").unwrap(),
            4 => File::options()
                .write(true)
                .open(&target)
                .unwrap()
                .set_len((HEADER_BYTES + MAX_PAYLOAD_BYTES + 1) as u64)
                .unwrap(),
            _ => {
                let mut bytes = fs::read(&target).unwrap();
                bytes[HEADER_BYTES] ^= 1;
                fs::write(&target, bytes).unwrap();
            }
        }
        let before = fs::read(&target).unwrap();
        assert!(owner.get(OBJECT).is_err(), "mutation {mutation}");
        assert!(owner.put(OBJECT, b"repair").is_err());
        assert_eq!(fs::read(&target).unwrap(), before);
    }
}

#[test]
fn typed_identifiers_preserve_zero_and_maximum_names_without_traversal_or_marker_collision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    for object in [
        ObjectId::from_bytes([0; 16]),
        ObjectId::from_bytes([255; 16]),
    ] {
        owner.put(object, &[]).unwrap();
        assert!(owner.get(object).unwrap().payload().is_empty());
        assert_eq!(name(object).len(), 39);
    }
    assert!(owner.put(OBJECT, &vec![0; MAX_PAYLOAD_BYTES + 1]).is_err());
    assert!(!path.join(name(OBJECT)).exists());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 3);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn competing_openers_select_one_owner_and_leave_original_object_bytes_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    directory(&path);
    drop(ProjectDirectory::initialize(&path, PROJECT).unwrap());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let owners = std::thread::scope(|scope| {
        let a = barrier.clone();
        let p = &path;
        let first = scope.spawn(move || {
            a.wait();
            ProjectDirectory::open(p, PROJECT)
        });
        let a = barrier.clone();
        let p = &path;
        let second = scope.spawn(move || {
            a.wait();
            ProjectDirectory::open(p, PROJECT)
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(owners.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        owners
            .iter()
            .filter(|r| matches!(r, Err(Error::Busy)))
            .count(),
        1
    );
    drop(owners);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    ProjectDirectory::open(&path, PROJECT).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_immutable_directory_histories_match_a_byte_map(
        events in prop::collection::vec((0u8..8,prop::collection::vec(any::<u8>(),0..1025)),1..17)
    ) {
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("objects");directory(&path);
        let mut owner=ProjectDirectory::initialize(&path,PROJECT).unwrap();let mut model=std::collections::BTreeMap::new();
        for (key,payload) in events {
            let object=ObjectId::from_bytes([key;16]);
            let result=owner.put(object,&payload);
            if let std::collections::btree_map::Entry::Vacant(entry)=model.entry(key) { result.unwrap();entry.insert(payload); }
            else { prop_assert!(result.is_err()); }
            drop(owner);owner=ProjectDirectory::open(&path,PROJECT).unwrap();
            for (&key,expected) in &model {
                let data=owner.get(ObjectId::from_bytes([key;16])).unwrap();
                prop_assert_eq!(data.payload(),expected);
            }
        }
        prop_assert_eq!(fs::read_dir(path).unwrap().count(),model.len()+1);
    }
}
