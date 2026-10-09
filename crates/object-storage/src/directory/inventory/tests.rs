use super::*;
use proptest::prelude::*;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn initialize(path: &Path, project: ProjectId) -> ProjectDirectory {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    ProjectDirectory::initialize(path, project).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn inventory_is_complete_sorted_repeatable_and_matches_independent_digest_vector() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    let mut owner = initialize(&path, PROJECT);
    let empty = owner.inventory().unwrap();
    assert!(empty.entries().is_empty());
    assert_eq!(empty.payload_bytes(), 0);
    let high = ObjectId::from_bytes([255; 16]);
    owner.put(high, b"ab").unwrap();
    owner.put(OBJECT, b"\0").unwrap();
    let before = fs::read(path.join(object_name(OBJECT))).unwrap();
    let checked = owner.inventory().unwrap();
    assert_eq!(checked.project(), PROJECT);
    assert_eq!(checked.payload_bytes(), 3);
    assert_eq!(checked.entries().len(), 2);
    assert_eq!(checked.entries()[0].object(), OBJECT);
    assert_eq!(checked.entries()[1].object(), high);
    assert_eq!(
        checked.entries()[0].report(),
        owner.get(OBJECT).unwrap().report()
    );
    // Independent Python hashlib + struct oracle, including exact domain framing.
    assert_eq!(
        hex(checked.digest()),
        "e7242882dc607385a855eb3e8afce91e57f027ff1361b6dc343b7a55f1468085"
    );
    for _ in 0..3 {
        assert_eq!(owner.inventory().unwrap(), checked);
    }
    assert_eq!(fs::read(path.join(object_name(OBJECT))).unwrap(), before);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 3);
    assert_ne!(empty.digest(), checked.digest());
}

#[test]
fn inventory_digest_is_order_independent_but_binds_project_ids_and_complete_contents() {
    let temp = tempfile::tempdir().unwrap();
    let mut a = initialize(&temp.path().join("a"), PROJECT);
    let mut b = initialize(&temp.path().join("b"), PROJECT);
    let mut foreign = initialize(&temp.path().join("foreign"), ProjectId::from_bytes([3; 16]));
    for key in 0..8 {
        a.put(ObjectId::from_bytes([key; 16]), &[key; 16]).unwrap();
    }
    for key in (0..8).rev() {
        b.put(ObjectId::from_bytes([key; 16]), &[key; 16]).unwrap();
        foreign
            .put(ObjectId::from_bytes([key; 16]), &[key; 16])
            .unwrap();
    }
    assert_eq!(a.inventory().unwrap(), b.inventory().unwrap());
    assert_ne!(
        a.inventory().unwrap().digest(),
        foreign.inventory().unwrap().digest()
    );
    b.put(ObjectId::from_bytes([9; 16]), &[]).unwrap();
    assert_ne!(
        a.inventory().unwrap().digest(),
        b.inventory().unwrap().digest()
    );
}

#[test]
fn every_single_byte_name_substitution_follows_canonical_ascii_and_suffix_rules() {
    let original = object_name(ObjectId::from_bytes([0; 16])).into_bytes();
    for end in 0..original.len() {
        assert!(object_id_from_name(&original[..end]).is_err());
    }
    for index in 0..39 {
        for byte in 0u8..=255 {
            let mut changed = original.clone();
            changed[index] = byte;
            let valid = if index < 32 {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            } else {
                byte == original[index]
            };
            assert_eq!(
                object_id_from_name(&changed).is_ok(),
                valid,
                "index {index}/byte {byte}"
            );
            if let Ok(id) = object_id_from_name(&changed) {
                assert_eq!(object_name(id).as_bytes(), changed);
            }
        }
    }
    for name in [
        b"../foreign.object".to_vec(),
        b"/absolute.object".to_vec(),
        vec![0xff; 39],
        vec![b'a'; 1048576],
    ] {
        assert!(object_id_from_name(&name).is_err());
    }
}

#[test]
fn unknown_staging_non_utf8_and_directory_entries_refuse_without_partial_inventory_or_cleanup() {
    for unknown in [
        "unexpected",
        ".emilybase-create-synthetic",
        "00000000000000000000000000000000.OBJECT",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        let mut owner = initialize(&path, PROJECT);
        owner.put(OBJECT, b"synthetic-original").unwrap();
        let unknown_path = path.join(unknown);
        fs::write(&unknown_path, b"synthetic-unmanaged").unwrap();
        assert!(matches!(owner.inventory(), Err(Error::Inventory)));
        assert_eq!(fs::read(unknown_path).unwrap(), b"synthetic-unmanaged");
        assert_eq!(owner.get(OBJECT).unwrap().payload(), b"synthetic-original");
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    let owner = initialize(&path, PROJECT);
    let unknown = path.join(std::ffi::OsString::from_vec(vec![0xff; 39]));
    fs::write(&unknown, b"synthetic").unwrap();
    assert!(owner.inventory().is_err());
    fs::remove_file(unknown).unwrap();
    fs::create_dir(path.join(object_name(OBJECT))).unwrap();
    assert!(owner.inventory().is_err());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
}

#[test]
fn complete_inventory_rejects_valid_foreign_images_aliases_permissions_and_payload_corruption() {
    for mutation in 0..5 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        let mut owner = initialize(&path, PROJECT);
        owner.put(OBJECT, b"synthetic-original").unwrap();
        let target = path.join(object_name(OBJECT));
        match mutation {
            0 => fs::write(
                &target,
                encode(ProjectId::from_bytes([3; 16]), OBJECT, b"synthetic-foreign").unwrap(),
            )
            .unwrap(),
            1 => fs::hard_link(&target, temp.path().join("alias")).unwrap(),
            2 => {
                fs::rename(&target, temp.path().join("detached")).unwrap();
                symlink(temp.path().join("detached"), &target).unwrap();
            }
            3 => fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap(),
            _ => {
                let mut bytes = fs::read(&target).unwrap();
                bytes[HEADER_BYTES] ^= 1;
                fs::write(&target, bytes).unwrap();
            }
        }
        let before = fs::read(&target).unwrap();
        assert!(owner.inventory().is_err());
        assert_eq!(fs::read(&target).unwrap(), before);
    }
}

#[test]
fn count_bound_accepts_exactly_128_objects_and_refuses_next_without_enforcing_put_quota() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    let mut owner = initialize(&path, PROJECT);
    for key in 0..MAX_INVENTORY_OBJECTS {
        owner
            .put(ObjectId::from_bytes([key as u8; 16]), &[])
            .unwrap();
    }
    let inventory = owner.inventory().unwrap();
    assert_eq!(inventory.entries().len(), MAX_INVENTORY_OBJECTS);
    assert_eq!(inventory.payload_bytes(), 0);
    owner.put(ObjectId::from_bytes([128; 16]), &[]).unwrap();
    assert!(matches!(owner.inventory(), Err(Error::Limit)));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 130);
    assert!(
        owner
            .get(ObjectId::from_bytes([128; 16]))
            .unwrap()
            .payload()
            .is_empty()
    );
}

#[test]
fn total_byte_bound_accepts_64_mib_and_refuses_an_additional_verified_byte() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    let mut owner = initialize(&path, PROJECT);
    let payload = vec![0x59; MAX_PAYLOAD_BYTES];
    for key in 0..8 {
        owner
            .put(ObjectId::from_bytes([key; 16]), &payload)
            .unwrap();
    }
    let inventory = owner.inventory().unwrap();
    assert_eq!(inventory.payload_bytes(), MAX_INVENTORY_BYTES);
    assert_eq!(inventory.entries().len(), 8);
    let snapshot = owner.capture_inventory(&inventory).unwrap();
    assert_eq!(snapshot.objects().len(), 8);
    assert_eq!(
        snapshot
            .objects()
            .iter()
            .map(|data| data.payload().len() as u64)
            .sum::<u64>(),
        MAX_INVENTORY_BYTES
    );
    assert!(
        snapshot
            .objects()
            .iter()
            .all(|data| data.payload() == payload)
    );
    drop(snapshot);
    owner.put(ObjectId::from_bytes([8; 16]), b"x").unwrap();
    assert!(matches!(owner.inventory(), Err(Error::Limit)));
    assert_eq!(fs::read_dir(&path).unwrap().count(), 10);
}

#[test]
fn two_name_scans_and_retained_image_receipts_refuse_mutations_between_verification_phases() {
    for mutation in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        let mut owner = initialize(&path, PROJECT);
        owner.put(OBJECT, b"old!").unwrap();
        let target = path.join(object_name(OBJECT));
        let result = owner.inventory_with(
            || {},
            || match mutation {
                0 => {
                    crate::publish_file(
                        path.join(object_name(ObjectId::from_bytes([4; 16]))),
                        PROJECT,
                        ObjectId::from_bytes([4; 16]),
                        b"new",
                    )
                    .unwrap();
                }
                1 => fs::remove_file(&target).unwrap(),
                2 => {
                    fs::rename(&target, temp.path().join("detached")).unwrap();
                    crate::publish_file(&target, PROJECT, OBJECT, b"old!").unwrap();
                }
                3 => fs::write(&target, encode(PROJECT, OBJECT, b"new!").unwrap()).unwrap(),
                4 => fs::write(path.join(SCOPE_FILE), b"damaged").unwrap(),
                _ => fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap(),
            },
        );
        assert!(result.is_err(), "mutation {mutation}");
        if mutation == 3 {
            assert_eq!(
                fs::read(&target).unwrap(),
                encode(PROJECT, OBJECT, b"new!").unwrap()
            );
        }
    }
}

#[test]
fn changed_name_after_initial_scan_refuses_and_namespace_moves_keep_original_inventory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    let mut owner = initialize(&path, PROJECT);
    owner.put(OBJECT, b"synthetic-original").unwrap();
    let expected = owner.inventory().unwrap();
    let moved = temp.path().join("moved");
    let checked = owner
        .inventory_with(
            || {
                fs::rename(&path, &moved).unwrap();
                fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            },
            || {},
        )
        .unwrap();
    assert_eq!(checked, expected);
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    let result = owner.inventory_with(
        || {
            fs::remove_file(moved.join(object_name(OBJECT))).unwrap();
        },
        || {},
    );
    assert!(result.is_err());
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 1);
}

#[test]
fn immutable_capture_retains_exact_bytes_after_owner_drop_and_later_filesystem_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    let mut owner = initialize(&path, PROJECT);
    owner.put(OBJECT, b"synthetic-private-before").unwrap();
    let zero = ObjectId::from_bytes([0; 16]);
    owner.put(zero, &[]).unwrap();
    let inventory = owner.inventory().unwrap();
    let captured = owner.capture_inventory(&inventory).unwrap();
    assert_eq!(captured.inventory(), &inventory);
    assert_eq!(captured.objects().len(), 2);
    assert_eq!(captured.objects()[0].object(), zero);
    assert_eq!(captured.objects()[1].payload(), b"synthetic-private-before");
    assert!(!format!("{captured:?}").contains("synthetic-private"));
    owner.put(ObjectId::from_bytes([4; 16]), b"new").unwrap();
    assert!(matches!(
        owner.capture_inventory(&inventory),
        Err(Error::InventoryChanged)
    ));
    let fresh = owner.capture().unwrap();
    assert_eq!(fresh.objects().len(), 3);
    drop(owner);
    fs::write(
        path.join(object_name(OBJECT)),
        encode(PROJECT, OBJECT, b"synthetic-private-after").unwrap(),
    )
    .unwrap();
    assert_eq!(captured.objects()[1].payload(), b"synthetic-private-before");
    assert_eq!(fresh.objects()[1].payload(), b"synthetic-private-before");
    assert_eq!(captured.inventory(), &inventory);
}

#[test]
fn checked_capture_receipt_binds_contents_and_project_without_granting_directory_authority() {
    let temp = tempfile::tempdir().unwrap();
    let mut first = initialize(&temp.path().join("a"), PROJECT);
    let mut same_project = initialize(&temp.path().join("b"), PROJECT);
    let mut foreign = initialize(&temp.path().join("c"), ProjectId::from_bytes([3; 16]));
    for owner in [&mut first, &mut same_project, &mut foreign] {
        owner.put(OBJECT, b"synthetic-equal").unwrap();
    }
    let receipt = first.inventory().unwrap();
    // Equivalent independently verified content can match; the receipt is metadata.
    assert_eq!(
        same_project
            .capture_inventory(&receipt)
            .unwrap()
            .inventory(),
        &receipt
    );
    assert!(matches!(
        foreign.capture_inventory(&receipt),
        Err(Error::Scope)
    ));
    same_project
        .put(ObjectId::from_bytes([4; 16]), &[])
        .unwrap();
    assert!(matches!(
        same_project.capture_inventory(&receipt),
        Err(Error::InventoryChanged)
    ));
    assert_eq!(
        first.capture_inventory(&receipt).unwrap().objects()[0].payload(),
        b"synthetic-equal"
    );
}

#[test]
fn capture_rechecks_data_and_complete_inventory_after_both_mutation_boundaries() {
    for mutation in 0..5 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("objects");
        let mut owner = initialize(&path, PROJECT);
        owner.put(OBJECT, b"old!").unwrap();
        let inventory = owner.inventory().unwrap();
        let target = path.join(object_name(OBJECT));
        let change = || match mutation {
            0 | 1 => fs::write(&target, encode(PROJECT, OBJECT, b"new!").unwrap()).unwrap(),
            2 => fs::remove_file(&target).unwrap(),
            3 => fs::write(path.join("unmanaged"), b"synthetic-unmanaged").unwrap(),
            _ => fs::write(path.join(SCOPE_FILE), b"damaged").unwrap(),
        };
        let result = if mutation == 0 {
            owner.capture_with(inventory, change, || {})
        } else {
            owner.capture_with(inventory, || {}, change)
        };
        assert!(result.is_err(), "mutation {mutation}");
        if mutation < 2 {
            assert_eq!(
                fs::read(&target).unwrap(),
                encode(PROJECT, OBJECT, b"new!").unwrap()
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_inventory_matches_independent_sorted_metadata_map(
        values in prop::collection::btree_map(0u8..16,prop::collection::vec(any::<u8>(),0..257),0..9)
    ) {
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("objects");let mut owner=initialize(&path,PROJECT);
        for (&key,value) in values.iter().rev() {owner.put(ObjectId::from_bytes([key;16]),value).unwrap();}
        let inventory=owner.inventory().unwrap();prop_assert_eq!(inventory.entries().len(),values.len());
        prop_assert_eq!(inventory.payload_bytes(),values.values().map(|v|v.len() as u64).sum::<u64>());
        for (entry,(&key,value)) in inventory.entries().iter().zip(&values) {
            prop_assert_eq!(entry.object(),ObjectId::from_bytes([key;16]));
            prop_assert_eq!(entry.report().payload_bytes,value.len());
            prop_assert_eq!(entry.report().sha256,<[u8;32]>::from(Sha256::digest(value)));
        }
        let snapshot=owner.capture_inventory(&inventory).unwrap();
        prop_assert_eq!(snapshot.inventory(),&inventory);
        for (data,(&key,value)) in snapshot.objects().iter().zip(&values) {
            prop_assert_eq!(data.object(),ObjectId::from_bytes([key;16]));prop_assert_eq!(data.payload(),value);
        }
        drop(owner);let reopened=ProjectDirectory::open(&path,PROJECT).unwrap();prop_assert_eq!(reopened.inventory().unwrap(),inventory);
    }
    #[test]
    fn generated_filename_round_trips_keep_all_identity_bytes_and_cannot_escape_directory(id in any::<[u8;16]>()) {
        let object=ObjectId::from_bytes(id);let name=object_name(object);
        prop_assert_eq!(object_id_from_name(name.as_bytes()).unwrap(),object);
        prop_assert_eq!(std::path::Path::new(&name).components().count(),1);
    }
}
