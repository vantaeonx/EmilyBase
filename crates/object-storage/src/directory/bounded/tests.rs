use super::*;
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn fixture() -> (tempfile::TempDir, PathBuf, ProjectDirectory) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    (temp, path, owner)
}

#[test]
fn selected_bounded_receipt_rejects_identical_replacement_after_inner_put_returns() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, path, mut owner) = fixture();
    let target = path.join(object_name(OBJECT));
    let saved = temp.path().join("original");
    let result = owner.put_bounded_with(
        OBJECT,
        b"synthetic-private",
        WriteLimits::new(1, 17).unwrap(),
        || {},
        || {
            fs::rename(&target, &saved).unwrap();
            fs::copy(&saved, &target).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        },
    );
    assert!(matches!(result, Err(Error::PublicationUnknown)));
    assert_eq!(fs::read(&target).unwrap(), fs::read(&saved).unwrap());
    assert_ne!(
        fs::metadata(&target).unwrap().ino(),
        fs::metadata(&saved).unwrap().ino()
    );
}

#[test]
fn limits_validate_exact_native_bounds_and_metadata_checks_never_reserve_authority() {
    let (_temp, _path, owner) = fixture();
    let inventory = owner.inventory().unwrap();
    assert!(matches!(WriteLimits::new(129, 0), Err(Error::WriteLimits)));
    assert!(matches!(
        WriteLimits::new(0, u64::MAX),
        Err(Error::WriteLimits)
    ));
    assert!(matches!(
        WriteLimits::new(1, MAX_INVENTORY_BYTES + 1),
        Err(Error::WriteLimits)
    ));
    let maximum = WriteLimits::new(128, MAX_INVENTORY_BYTES).unwrap();
    assert_eq!(maximum.objects(), 128);
    assert_eq!(maximum.payload_bytes(), MAX_INVENTORY_BYTES);
    assert!(matches!(
        maximum.check(&inventory, OBJECT, usize::MAX),
        Err(Error::Limit)
    ));
    assert!(
        WriteLimits::new(1, 0)
            .unwrap()
            .check(&inventory, OBJECT, 0)
            .is_ok()
    );
    assert!(matches!(
        WriteLimits::new(0, 0).unwrap().check(&inventory, OBJECT, 0),
        Err(Error::Limit)
    ));
    assert!(matches!(
        WriteLimits::new(1, 0).unwrap().check(&inventory, OBJECT, 1),
        Err(Error::Limit)
    ));
    assert_eq!(owner.inventory().unwrap(), inventory);
}

#[test]
fn exact_binary_writes_return_complete_sorted_receipts_and_refuse_duplicate_or_capacity_overflow() {
    let (_temp, path, mut owner) = fixture();
    let limits = WriteLimits::new(3, 4).unwrap();
    let first = owner
        .put_bounded(ObjectId::from_bytes([4; 16]), b"\0\xff", limits)
        .unwrap();
    assert_eq!(first.report().payload_bytes, 2);
    let receipt = owner.put_bounded(OBJECT, b"\xfe\n", limits).unwrap();
    assert_eq!(receipt.object(), OBJECT);
    assert_eq!(
        receipt.report().sha256,
        Sha256::digest(b"\xfe\n").as_slice()
    );
    assert_eq!(receipt.inventory().payload_bytes(), 4);
    assert_eq!(receipt.inventory().entries().len(), 2);
    assert_eq!(receipt.inventory().entries()[0].object(), OBJECT);
    assert_eq!(*receipt.inventory(), owner.inventory().unwrap());
    let original = fs::read(path.join(object_name(OBJECT))).unwrap();
    assert!(matches!(
        owner.put_bounded(OBJECT, b"replacement", limits),
        Err(Error::Exists)
    ));
    assert!(matches!(
        owner.put_bounded(ObjectId::from_bytes([5; 16]), b"x", limits),
        Err(Error::Limit)
    ));
    let empty = owner
        .put_bounded(ObjectId::from_bytes([0; 16]), &[], limits)
        .unwrap();
    assert_eq!(empty.inventory().entries().len(), 3);
    assert!(matches!(
        owner.put_bounded(ObjectId::from_bytes([6; 16]), &[], limits),
        Err(Error::Limit)
    ));
    assert_eq!(fs::read(path.join(object_name(OBJECT))).unwrap(), original);
    assert_eq!(fs::read_dir(path).unwrap().count(), 4);
}

#[test]
fn lowering_per_call_limits_does_not_evict_existing_data_or_create_a_refused_stage() {
    let (_temp, path, mut owner) = fixture();
    let receipt = owner
        .put_bounded(
            OBJECT,
            b"synthetic-private",
            WriteLimits::new(128, MAX_INVENTORY_BYTES).unwrap(),
        )
        .unwrap();
    assert!(!format!("{receipt:?}").contains("synthetic-private"));
    let before = owner.inventory().unwrap();
    for limits in [
        WriteLimits::new(0, 0).unwrap(),
        WriteLimits::new(1, 64).unwrap(),
        WriteLimits::new(128, 0).unwrap(),
    ] {
        assert!(matches!(
            owner.put_bounded(ObjectId::from_bytes([4; 16]), &[], limits),
            Err(Error::Limit)
        ));
        assert_eq!(owner.inventory().unwrap(), before);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 2);
    }
}

#[test]
fn unmanaged_or_corrupt_source_refuses_complete_admission_without_writing_new_name() {
    for mutation in 0..3 {
        let (_temp, path, mut owner) = fixture();
        owner.put(OBJECT, b"synthetic-private").unwrap();
        match mutation {
            0 => fs::write(path.join("unknown"), b"synthetic-foreign").unwrap(),
            1 => fs::write(path.join(object_name(OBJECT)), b"damaged").unwrap(),
            _ => fs::write(path.join(SCOPE_FILE), b"damaged").unwrap(),
        }
        let before = fs::read(path.join(object_name(OBJECT))).unwrap();
        let count = fs::read_dir(&path).unwrap().count();
        let new = ObjectId::from_bytes([4; 16]);
        assert!(
            owner
                .put_bounded(
                    new,
                    &[],
                    WriteLimits::new(128, MAX_INVENTORY_BYTES).unwrap()
                )
                .is_err()
        );
        assert!(!path.join(object_name(new)).exists());
        assert_eq!(fs::read(path.join(object_name(OBJECT))).unwrap(), before);
        assert_eq!(fs::read_dir(path).unwrap().count(), count);
    }
}

#[test]
fn delayed_source_change_refuses_before_selection_and_preserves_foreign_work() {
    let (_temp, path, mut owner) = fixture();
    owner.put(OBJECT, b"old!").unwrap();
    let new = ObjectId::from_bytes([4; 16]);
    let result = owner.put_bounded_with(
        new,
        b"new!",
        WriteLimits::new(2, 8).unwrap(),
        || {
            fs::write(
                path.join(object_name(OBJECT)),
                encode(PROJECT, OBJECT, b"more").unwrap(),
            )
            .unwrap()
        },
        || panic!("must not select"),
    );
    assert!(matches!(result, Err(Error::InventoryChanged)));
    assert!(!path.join(object_name(new)).exists());
    assert_eq!(owner.get(OBJECT).unwrap().payload(), b"more");
}

#[test]
fn postselection_contents_or_inventory_change_is_unknown_and_never_swept() {
    for mutation in 0..3 {
        let (_temp, path, mut owner) = fixture();
        owner.put(ObjectId::from_bytes([4; 16]), b"old!").unwrap();
        let result = owner.put_bounded_with(
            OBJECT,
            b"new!",
            WriteLimits::new(2, 8).unwrap(),
            || {},
            || match mutation {
                0 => fs::write(path.join(object_name(OBJECT)), b"damaged").unwrap(),
                1 => fs::write(path.join("unknown"), b"synthetic-foreign").unwrap(),
                _ => fs::write(
                    path.join(object_name(ObjectId::from_bytes([4; 16]))),
                    encode(PROJECT, ObjectId::from_bytes([4; 16]), b"more").unwrap(),
                )
                .unwrap(),
            },
        );
        assert!(matches!(result, Err(Error::PublicationUnknown)));
        assert!(path.join(object_name(OBJECT)).exists());
        if mutation != 0 {
            assert_eq!(owner.get(OBJECT).unwrap().payload(), b"new!");
        }
    }
}

#[test]
fn retained_source_namespace_and_independent_project_limits_do_not_redirect_or_share_capacity() {
    let (temp, path, mut owner) = fixture();
    let moved = temp.path().join("moved");
    let limits = WriteLimits::new(1, 4).unwrap();
    owner
        .put_bounded_with(
            OBJECT,
            b"full",
            limits,
            || {
                fs::rename(&path, &moved).unwrap();
                fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            },
            || {},
        )
        .unwrap();
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    assert!(moved.join(object_name(OBJECT)).exists());
    let mut foreign = ProjectDirectory::initialize(&path, ProjectId::from_bytes([9; 16])).unwrap();
    foreign.put_bounded(OBJECT, b"more", limits).unwrap();
    assert_eq!(owner.get(OBJECT).unwrap().payload(), b"full");
    assert_eq!(foreign.get(OBJECT).unwrap().payload(), b"more");
    assert!(matches!(
        owner.put_bounded(ObjectId::from_bytes([4; 16]), &[], limits),
        Err(Error::Limit)
    ));
}

#[test]
fn physical_count_limit_accepts_128_empty_objects_and_refuses_next_before_staging() {
    let (_temp, path, mut owner) = fixture();
    let limits = WriteLimits::new(128, 0).unwrap();
    for key in 0..128 {
        let result = owner
            .put_bounded(ObjectId::from_bytes([key; 16]), &[], limits)
            .unwrap();
        assert_eq!(result.inventory().entries().len(), key as usize + 1);
    }
    assert!(matches!(
        owner.put_bounded(ObjectId::from_bytes([128; 16]), &[], limits),
        Err(Error::Limit)
    ));
    assert_eq!(fs::read_dir(path).unwrap().count(), 129);
}

#[test]
fn exact_64_mib_capacity_admits_last_full_object_and_empty_name_but_refuses_next_byte() {
    let (_temp, path, mut owner) = fixture();
    let payload = vec![0x59; MAX_PAYLOAD_BYTES];
    for key in 0..7 {
        owner
            .put(ObjectId::from_bytes([key; 16]), &payload)
            .unwrap();
    }
    let limits = WriteLimits::new(128, MAX_INVENTORY_BYTES).unwrap();
    let result = owner
        .put_bounded(ObjectId::from_bytes([7; 16]), &payload, limits)
        .unwrap();
    assert_eq!(result.inventory().payload_bytes(), MAX_INVENTORY_BYTES);
    assert!(matches!(
        owner.put_bounded(ObjectId::from_bytes([8; 16]), b"x", limits),
        Err(Error::Limit)
    ));
    assert!(
        !path
            .join(object_name(ObjectId::from_bytes([8; 16])))
            .exists()
    );
    let result = owner
        .put_bounded(ObjectId::from_bytes([8; 16]), &[], limits)
        .unwrap();
    assert_eq!(result.inventory().entries().len(), 9);
    assert_eq!(result.inventory().payload_bytes(), MAX_INVENTORY_BYTES);
}

#[test]
fn serialized_competing_callers_admit_exact_capacity_without_volatile_counters() {
    let (_temp, path, owner) = fixture();
    let owner = std::sync::Mutex::new(owner);
    let barrier = std::sync::Barrier::new(8);
    let limits = WriteLimits::new(2, 2).unwrap();
    let results = std::thread::scope(|scope| {
        let workers = (0..8)
            .map(|key| {
                let owner = &owner;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    owner.lock().unwrap().put_bounded(
                        ObjectId::from_bytes([key; 16]),
                        &[key],
                        limits,
                    )
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 2);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Limit)))
            .count(),
        6
    );
    drop(owner);
    let owner = ProjectDirectory::open(path, PROJECT).unwrap();
    let inventory = owner.inventory().unwrap();
    assert_eq!(inventory.entries().len(), 2);
    assert_eq!(inventory.payload_bytes(), 2);
    for entry in inventory.entries() {
        assert_eq!(
            owner.get(entry.object()).unwrap().payload(),
            &entry.object().as_bytes()[..1]
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_capacity_histories_follow_independent_map_and_byte_sum(
        objects in 0usize..9, bytes in 0u64..129,
        operations in prop::collection::vec((0u8..12,prop::collection::vec(any::<u8>(),0..33)),0..25)
    ) {
        let (_temp,path,mut owner)=fixture();let limits=WriteLimits::new(objects,bytes).unwrap();let mut model=std::collections::BTreeMap::<u8,Vec<u8>>::new();
        for (key,value) in operations {
            let id=ObjectId::from_bytes([key;16]);let duplicate=model.contains_key(&key);let total=model.values().map(|value|value.len() as u64).sum::<u64>();let admit=!duplicate && model.len()<objects && total+value.len() as u64<=bytes;
            let result=owner.put_bounded(id,&value,limits);
            if admit {let receipt=result.unwrap();model.insert(key,value);prop_assert_eq!(receipt.inventory().entries().len(),model.len());prop_assert_eq!(receipt.inventory().payload_bytes(),model.values().map(|value|value.len() as u64).sum::<u64>());}
            else if duplicate {prop_assert!(matches!(result,Err(Error::Exists)));} else {prop_assert!(matches!(result,Err(Error::Limit)));}
            prop_assert_eq!(fs::read_dir(&path).unwrap().count(),model.len()+1);
            let inventory=owner.inventory().unwrap();prop_assert_eq!(inventory.entries().len(),model.len());
            for (&key,value) in &model {let stored=owner.get(ObjectId::from_bytes([key;16])).unwrap();prop_assert_eq!(stored.payload(),value);}
        }
    }
}
