use super::*;

#[test]
fn own_password_change_derives_identity_and_revokes_all_old_families_on_both_wals() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact, true);
        let (id, key) = f.store.credentials[0].clone();
        let first = f.first.access.expose().to_owned();
        let another = f.root.public_sign_in(&id, LOGIN, PASSWORD, 50).unwrap();
        let owner = f.root.public_user(&id, &first, 50).unwrap();
        let before = histories(&f.store);
        let rotated = f.root.rotate_project_key(&id).unwrap();
        // Metadata rotation is unrelated to user authority; compare after it.
        let baseline = histories(&f.store);
        let public = Database::open(f.store.root.join("registry").join(&id).join("data")).unwrap();
        let changed = f
            .root
            .public_change_password(&id, &first, PASSWORD, b"synthetic-replacement", 50)
            .unwrap();
        assert_eq!(changed.id, owner.id);
        assert_eq!(changed.login, LOGIN);
        assert_eq!(changed.credential_epoch, owner.credential_epoch + 1);
        assert!(!changed.disabled);
        for token in [first.as_str(), another.access.expose()] {
            assert!(matches!(
                f.root.public_user(&id, token, 50),
                Err(Error::Accounts(A::Denied))
            ));
        }
        for token in [f.first.refresh.expose(), another.refresh.expose()] {
            assert!(matches!(
                f.root.public_refresh_session(&id, token, 50),
                Err(Error::Accounts(A::Denied))
            ));
        }
        assert_eq!(
            f.root
                .public_user(&id, f.second.access.expose(), 50)
                .unwrap()
                .login,
            "second_user"
        );
        let after = histories(&f.store);
        for (i, (old, new)) in baseline.iter().zip(&after).enumerate() {
            if i != 3 {
                assert_eq!(old, new);
            }
        }
        assert_ne!(after[3], baseline[3]);
        assert_eq!(before[2], after[2]);
        drop(public);
        assert!(matches!(
            f.root.public_sign_in(&id, LOGIN, PASSWORD, 50),
            Err(Error::Accounts(A::Denied))
        ));
        let fresh = f
            .root
            .public_sign_in(&id, LOGIN, b"synthetic-replacement", 50)
            .unwrap();
        assert_eq!(
            f.root
                .public_user(&id, fresh.access.expose(), 50)
                .unwrap()
                .credential_epoch,
            2
        );
        assert!(f.root.list_users(&id, &key, None, 1).is_err());
        assert!(f.root.list_users(&id, &rotated.api_key, None, 1).is_ok());
    }
}

#[test]
fn own_password_change_rechecks_closed_legacy_scope_and_current_credentials_before_mutation() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = setup(dir.path(), compact, false);
        let (id, key) = f.store.credentials[0].clone();
        let before = histories(&f.store);
        assert!(matches!(
            f.root
                .public_change_password(&id, f.first.access.expose(), b"", b"", u64::MAX),
            Err(Error::Denied)
        ));
        assert_eq!(histories(&f.store), before);
        let closed = f.root.public_admission(&id, &key).unwrap();
        f.root
            .set_public_admission(&id, &key, closed.revision, true)
            .unwrap();
        let before = histories(&f.store);
        for token in [f.first.refresh.expose(), "invalid", &key] {
            assert!(
                f.root
                    .public_change_password(&id, token, PASSWORD, b"new", 50)
                    .is_err()
            );
        }
        for (current, replacement) in [
            (b"wrong".as_slice(), b"new".as_slice()),
            (PASSWORD, b""),
            (PASSWORD, &[0; 1025]),
        ] {
            assert!(
                f.root
                    .public_change_password(&id, f.first.access.expose(), current, replacement, 50)
                    .is_err()
            );
        }
        assert_eq!(histories(&f.store), before);
        let other = f.store.credentials[1].0.clone();
        assert!(matches!(
            f.root.public_change_password(
                &other,
                f.first.access.expose(),
                PASSWORD,
                b"new",
                u64::MAX
            ),
            Err(Error::Denied)
        ));
        f.root.set_disabled(&id, &key, LOGIN, true).unwrap();
        let disabled = histories(&f.store);
        assert!(matches!(
            f.root
                .public_change_password(&id, f.first.access.expose(), PASSWORD, b"new", 50),
            Err(Error::Accounts(A::Denied))
        ));
        assert_eq!(histories(&f.store), disabled);
        // A root without v5 never upgrades implicitly or consults invalid time.
        drop(f.root);
        fs::create_dir(dir.path().join("legacy")).unwrap();
        let legacy = restored(&dir.path().join("legacy"), 1, compact);
        let mut root = AccountRoot::open(&legacy.root, pool()).unwrap();
        let before = histories(&legacy);
        assert!(matches!(
            root.public_change_password(&legacy.credentials[0].0, "invalid", b"", b"", u64::MAX),
            Err(Error::Denied)
        ));
        assert_eq!(histories(&legacy), before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn generated_binary_password_histories_follow_original_epoch_model(
        compact in any::<bool>(),
        passwords in prop::collection::vec(prop::collection::vec(any::<u8>(),1..33),4)
    ) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();
        let dir=tempfile::tempdir().unwrap();
        let mut f=setup(dir.path(),compact,true);
        let id=f.store.credentials[0].0.clone();
        let public=histories(&f.store)[2].clone();
        let mut current=PASSWORD.to_vec();
        for (step,next) in passwords.iter().enumerate() {
            let pair=f.root.public_sign_in(&id,LOGIN,&current,50).unwrap();
            let previous=pair.access.expose().to_owned();
            let changed=f.root.public_change_password(&id,&previous,&current,next,50).unwrap();
            prop_assert_eq!(changed.credential_epoch,step as u64+2);
            prop_assert!(matches!(f.root.public_user(&id,&previous,50),Err(Error::Accounts(A::Denied))));
            prop_assert!(matches!(f.root.public_refresh_session(&id,pair.refresh.expose(),50),Err(Error::Accounts(A::Denied))));
            prop_assert!(f.root.public_sign_in(&id,LOGIN,next,50).is_ok());
            prop_assert_eq!(&histories(&f.store)[2],&public);
            current=next.clone();
        }
        prop_assert_eq!(f.root.public_user(&id,f.second.access.expose(),50).unwrap().login,"second_user");
    }
}
