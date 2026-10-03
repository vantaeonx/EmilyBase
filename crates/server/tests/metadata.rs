use emilybase_server::{Error, MAX_METADATA_BYTES, ProjectStore, inspect_project_metadata};
use proptest::prelude::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn rechecksum(envelope: &mut serde_json::Value) {
    let payload = &envelope["payload"];
    let ordered = format!(
        "{{\"version\":{},\"id\":{},\"name\":{},\"key\":{},\"epoch\":{}}}",
        payload["version"], payload["id"], payload["name"], payload["key"], payload["epoch"]
    );
    envelope["checksum"] = serde_json::json!(crc32fast::hash(ordered.as_bytes()));
}

#[test]
fn cut_metadata_and_semantic_damage_with_repaired_crc_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let created = store.create("Unicode 🌌").unwrap();
    let path = root.join(&created.project.id).join("project.json");
    let original = std::fs::read(&path).unwrap();
    for length in 0..original.len() {
        assert!(inspect_project_metadata(&created.project.id, &original[..length]).is_err());
    }
    let valid = inspect_project_metadata(&created.project.id, &original).unwrap();
    assert_eq!(valid.name, "Unicode 🌌");
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for (field, changed) in [
        ("version", serde_json::json!(2)),
        ("id", serde_json::json!("0".repeat(32))),
        ("name", serde_json::json!("control\nname")),
        ("name", serde_json::json!("")),
        ("epoch", serde_json::json!(0)),
        ("key", serde_json::json!(vec![0; 31])),
    ] {
        let mut changed_value = value.clone();
        changed_value["payload"][field] = changed;
        rechecksum(&mut changed_value);
        assert!(
            inspect_project_metadata(
                &created.project.id,
                &serde_json::to_vec(&changed_value).unwrap()
            )
            .is_err()
        );
    }
    let mut unknown = value.clone();
    unknown["payload"]["unexpected"] = serde_json::json!(true);
    rechecksum(&mut unknown);
    assert!(
        inspect_project_metadata(&created.project.id, &serde_json::to_vec(&unknown).unwrap())
            .is_err()
    );
    let mut envelope = value.clone();
    envelope["unexpected"] = serde_json::json!(true);
    assert!(
        inspect_project_metadata(&created.project.id, &serde_json::to_vec(&envelope).unwrap())
            .is_err()
    );
    assert!(inspect_project_metadata("../outside", &original).is_err());
    assert!(
        inspect_project_metadata(
            &created.project.id,
            &vec![b' '; MAX_METADATA_BYTES as usize + 1]
        )
        .is_err()
    );
    let mut damage = value;
    damage["payload"]["name"] = serde_json::json!("valid different name");
    std::fs::write(&path, serde_json::to_vec(&damage).unwrap()).unwrap();
    drop(store);
    assert!(matches!(ProjectStore::open(&root), Err(Error::Metadata)));
    std::fs::write(&path, original).unwrap();
    assert!(ProjectStore::open(&root).is_ok());
}

#[test]
fn epoch_overflow_and_metadata_symlink_do_not_rotate_or_touch_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let created = store.create("boundary").unwrap();
    let path = root.join(&created.project.id).join("project.json");
    drop(store);
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    metadata["payload"]["epoch"] = serde_json::json!(u64::MAX);
    rechecksum(&mut metadata);
    let original = serde_json::to_vec(&metadata).unwrap();
    std::fs::write(&path, &original).unwrap();
    let mut store = ProjectStore::open(&root).unwrap();
    assert!(matches!(
        store.rotate(&created.project.id),
        Err(Error::Limit)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let fresh = store.create("symlink").unwrap();
    let path = root.join(&fresh.project.id).join("project.json");
    let safe = dir.path().join("untouched");
    std::fs::write(&safe, b"synthetic target").unwrap();
    std::fs::remove_file(&path).unwrap();
    symlink(&safe, &path).unwrap();
    assert!(matches!(store.rotate(&fresh.project.id), Err(Error::Path)));
    assert_eq!(std::fs::read(&safe).unwrap(), b"synthetic target");
    drop(store);
    assert!(matches!(ProjectStore::open(&root), Err(Error::Path)));
}

#[test]
fn orphan_staging_is_not_adopted_and_private_permissions_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects");
    let mut store = ProjectStore::open(&root).unwrap();
    let created = store.create("private").unwrap();
    drop(store);
    let pending = root.join(".creating-interrupted");
    std::fs::create_dir(&pending).unwrap();
    std::fs::write(pending.join("project.json"), b"invalid incomplete metadata").unwrap();
    let store = ProjectStore::open(&root).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    drop(store);
    assert!(pending.exists());
    let metadata = root.join(&created.project.id).join("project.json");
    assert_eq!(
        std::fs::metadata(&metadata).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(ProjectStore::open(&root), Err(Error::Path)));
    std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(ProjectStore::open(&root).is_ok());
    std::fs::write(root.join("unrecognized"), b"untouched").unwrap();
    assert!(matches!(ProjectStore::open(&root), Err(Error::Path)));
    assert_eq!(
        std::fs::read(root.join("unrecognized")).unwrap(),
        b"untouched"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn repaired_crc_does_not_bypass_generated_semantic_boundaries(
        version in 0u16..3, epoch in prop_oneof![Just(0u64),Just(u64::MAX),1u64..100],
        name in prop_oneof!["[a-zé🌌]{0,40}",prop::collection::vec(any::<char>(),0..70).prop_map(|v|v.into_iter().collect::<String>())],
        matching in any::<bool>()
    ) {
        let id="0".repeat(32);let payload_id=if matching {id.clone()}else{"1".repeat(32)};
        let mut envelope=serde_json::json!({"payload":{"version":version,"id":payload_id,"name":name,"key":vec![0;32],"epoch":epoch},"checksum":0});
        rechecksum(&mut envelope);
        let expected=version==1 && epoch>0 && matching && !name.trim().is_empty() && name.len()<=128 && !name.chars().any(char::is_control);
        let result=inspect_project_metadata(&id,&serde_json::to_vec(&envelope).unwrap());
        prop_assert_eq!(result.is_ok(),expected);
        if let Ok(info)=result {prop_assert_eq!(info.id,id);prop_assert_eq!(info.name,name);prop_assert_eq!(info.key_epoch,epoch);}
    }
    #[test]
    fn arbitrary_metadata_bytes_are_bounded_and_never_panic(bytes in prop::collection::vec(any::<u8>(),0..5000)) {
        let id="0".repeat(32);
        let a=inspect_project_metadata(&id,&bytes);let b=inspect_project_metadata(&id,&bytes);
        prop_assert_eq!(a.is_ok(),b.is_ok());
        if bytes.len()>MAX_METADATA_BYTES as usize {prop_assert!(a.is_err());}
    }
}
