use emilybase_storage::{Page, Pager};

#[test]
fn initial_pages_are_published_with_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("initialized.emily");
    let mut page = Page::new(1).unwrap();
    page.insert(b"synthetic marker").unwrap();
    let db = Pager::create_with_pages(&path, &[page.clone()]).unwrap();
    assert_eq!(db.page_count(), 1);
    drop(db);
    let mut reopened = Pager::open(path).unwrap();
    assert_eq!(reopened.read_page(1).unwrap(), page);
}

#[test]
fn invalid_initial_pages_publish_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.emily");
    assert!(Pager::create_with_pages(&path, &[Page::new(2).unwrap()]).is_err());
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
