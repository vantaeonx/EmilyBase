use super::*;
use std::cell::RefCell;

#[test]
fn pager_owner_lifetime_releases_inherited_description_without_unlocking_its_successor() {
    let _serial = crate::publication_tests::CASES.lock().unwrap();
    for created in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("synthetic.emily");
        let mut page = Page::new(1).unwrap();
        page.insert(b"synthetic-owner-lifetime").unwrap();
        let first = Pager::create_with_pages(&path, std::slice::from_ref(&page)).unwrap();
        let owner = if created {
            first
        } else {
            drop(first);
            Pager::open(&path).unwrap()
        };
        // Model a fork-inherited open file description without unsafe fork or
        // pre-exec hooks. The duplicate is not another authorized Pager owner.
        let inherited = owner.file.try_clone().unwrap();
        assert!(matches!(Pager::open(&path), Err(Error::Busy)));
        drop(owner);
        let mut successor = Pager::open(&path).unwrap();
        assert_eq!(successor.read_page(1).unwrap(), page);
        drop(inherited);
        assert!(matches!(Pager::open(&path), Err(Error::Busy)));
        successor.write_page(&page).unwrap();
        drop(successor);
        Pager::open(&path).unwrap().verify().unwrap();
    }
}

#[test]
fn failed_open_releases_inherited_description_before_length_or_header_repair_and_reopen() {
    let _serial = crate::publication_tests::CASES.lock().unwrap();
    for shape in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("synthetic.emily");
        drop(Pager::create(&path).unwrap());
        let original = std::fs::read(&path).unwrap();
        let mut changed = original.clone();
        match shape {
            0 => changed.truncate(100),
            1 => changed[0] ^= 1,
            2 => changed[8] ^= 1,
            _ => changed[100] ^= 1,
        }
        std::fs::write(&path, changed).unwrap();
        let inherited = RefCell::new(None);
        let result = Pager::open_with(&path, |file| {
            *inherited.borrow_mut() = Some(file.try_clone().unwrap());
        });
        assert!(result.is_err());
        assert!(inherited.borrow().is_some());
        std::fs::write(&path, &original).unwrap();
        let mut successor = Pager::open(&path).unwrap();
        successor.verify().unwrap();
        drop(inherited.into_inner());
        assert!(matches!(Pager::open(&path), Err(Error::Busy)));
    }
}

#[test]
fn failed_creation_releases_private_selected_or_staged_description_with_original_outcomes() {
    use crate::publication_tests::FaultGuard;
    let _serial = crate::publication_tests::CASES.lock().unwrap();
    for (phase, after, selected) in [
        ("file_sync", false, false),
        ("file_sync", true, false),
        ("parent_sync", false, true),
        ("parent_sync", true, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("synthetic.emily");
        let _fault = FaultGuard::new(phase, after);
        let result = Pager::create(&path);
        if selected {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            let successor = Pager::open(&path).unwrap();
            assert!(matches!(Pager::open(&path), Err(Error::Busy)));
            drop(successor);
        } else {
            assert!(matches!(result, Err(Error::Io(_))));
            assert!(!path.exists());
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn poisoned_owner_preserves_write_refusal_and_releases_lock_on_drop() {
    let _serial = crate::publication_tests::CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("synthetic.emily");
    let mut page = Page::new(1).unwrap();
    page.insert(b"synthetic-original").unwrap();
    let mut owner = Pager::create_with_pages(&path, std::slice::from_ref(&page)).unwrap();
    let inherited = owner.file.try_clone().unwrap();
    // Removing a complete page forces the actual page reader's UnexpectedEof.
    inherited.set_len(PAGE_SIZE as u64).unwrap();
    assert!(matches!(owner.read_page(1), Err(Error::Io(_))));
    assert!(matches!(owner.write_page(&page), Err(Error::Poisoned)));
    drop(owner);
    let mut successor = Pager::open(&path).unwrap();
    assert_eq!(successor.page_count(), 0);
    successor.write_page(&page).unwrap();
    drop(inherited);
    assert!(matches!(Pager::open(&path), Err(Error::Busy)));
    assert_eq!(successor.read_page(1).unwrap(), page);
}
