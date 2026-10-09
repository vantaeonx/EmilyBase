use super::*;

#[test]
fn current_service_owner_controls_admission_and_verified_common_clone_closes_only_the_copy() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let f = restored(dir.path(), 2, compacted);
        let (id, key) = &f.credentials[0];
        let (sibling, sibling_key) = &f.credentials[1];
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let before = histories(&f);
        denied(root.enable_public_admission_catalog(id, sibling_key));
        denied(root.public_admission(id, sibling_key));
        denied(root.set_public_admission(id, sibling_key, 0, true));
        assert!(matches!(
            root.enable_public_admission_catalog(id, key),
            Err(Error::Accounts(
                emilybase_auth::accounts::Error::AdmissionSchema
            ))
        ));
        assert_eq!(histories(&f), before);
        root.enable_row_policy_catalog(id, key).unwrap();
        let closed = root.enable_public_admission_catalog(id, key).unwrap();
        assert!(!closed.enabled);
        let document=br#"{"version":1,"select":{"kind":"authenticated"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
        let policy = root.install_row_policy(id, key, "t", 0, document).unwrap();
        let session = root.sign_in(id, key, LOGIN, PASSWORD, 50).unwrap();
        let open = root
            .set_public_admission(id, key, closed.revision, true)
            .unwrap();
        assert!(open.enabled);
        let after = histories(&f);
        assert_eq!(before[0], after[0]);
        assert_eq!(&before[1..3], &after[1..3]);
        assert_eq!(&before[4..], &after[4..]);
        let rotated = root.rotate_project_key(id).unwrap();
        let before = histories(&f);
        denied(root.set_public_admission(id, key, open.revision, false));
        denied(root.public_admission(id, key));
        denied(root.set_public_admission(sibling, &rotated.api_key, 0, true));
        assert_eq!(histories(&f), before);
        assert_eq!(root.public_admission(id, &rotated.api_key).unwrap(), open);
        root.with_access(id, &rotated.api_key, session.access.expose(), 50, |_| ())
            .unwrap();
        drop(root);
        let before = histories(&f);
        let report = inspect_account_bundle_root(&f.root, pool()).unwrap();
        assert_eq!(
            report
                .private_accounts
                .iter()
                .find(|entry| entry.project == *id)
                .unwrap()
                .inventory
                .private_version,
            5
        );
        let bytes = crate::capture_account_bundle_root(&f.root, pool()).unwrap();
        assert_eq!(histories(&f), before);
        let target = dir.path().join("clone");
        let copy_report = restore_account_bundle_bytes(&bytes, &target, pool(), 40).unwrap();
        assert_eq!(copy_report.private_accounts.len(), 2);
        let mut copy = AccountRoot::open(&target, pool()).unwrap();
        let receipt = copy.public_admission(id, &rotated.api_key).unwrap();
        assert!(!receipt.enabled);
        assert_eq!(receipt.previous, open.revision);
        assert!(receipt.revision > open.revision);
        assert_eq!(
            copy.row_policy_receipts(id, &rotated.api_key).unwrap(),
            vec![policy]
        );
        assert!(
            copy.with_access(id, &rotated.api_key, session.access.expose(), 40, |_| ())
                .is_err()
        );
        let fresh = copy
            .sign_in(id, &rotated.api_key, LOGIN, PASSWORD, 40)
            .unwrap();
        copy.with_access(id, &rotated.api_key, fresh.access.expose(), 40, |_| ())
            .unwrap();
        assert!(matches!(
            copy.public_admission(sibling, sibling_key),
            Err(Error::Accounts(
                emilybase_auth::accounts::Error::AdmissionSchema
            ))
        ));
        assert_eq!(
            copy.execute(id, &rotated.api_key, "SELECT * FROM t", &[])
                .unwrap()
                .results[0]
                .rows
                .len(),
            1
        );
        drop(copy);
        assert_eq!(histories(&f), before);
        let mut source = AccountRoot::open(&f.root, pool()).unwrap();
        assert_eq!(source.public_admission(id, &rotated.api_key).unwrap(), open);
        source
            .with_access(id, &rotated.api_key, session.access.expose(), 50, |_| ())
            .unwrap();
    }
}
