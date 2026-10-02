use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

use emilybase_storage::{Error, PAGE_SIZE, Page, Pager};

#[test]
fn records_survive_reopen_update_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.emily");
    let mut db = Pager::create(&path).unwrap();
    let mut page = Page::new(1).unwrap();
    let a = page.insert(b"alpha").unwrap();
    let b = page.insert(b"beta").unwrap();
    db.write_page(&page).unwrap();
    drop(db);
    let mut db = Pager::open(&path).unwrap();
    let mut page = db.read_page(1).unwrap();
    assert_eq!(page.get(a).unwrap(), b"alpha");
    page.update(a, b"changed").unwrap();
    page.delete(b).unwrap();
    db.write_page(&page).unwrap();
    drop(db);
    let mut db = Pager::open(&path).unwrap();
    db.verify().unwrap();
    let page = db.read_page(1).unwrap();
    assert_eq!(page.get(a).unwrap(), b"changed");
    assert!(page.get(b).is_err());
    assert_eq!(db.page_count(), 1);
    assert_eq!(fs::metadata(path).unwrap().len(), 2 * PAGE_SIZE as u64);
}

#[test]
fn creation_never_overwrites_and_cleans_temporary_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.emily");
    fs::write(&path, b"keep me").unwrap();
    assert!(Pager::create(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"keep me");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn exclusive_lock_is_released_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.emily");
    let db = Pager::create(&path).unwrap();
    assert!(matches!(Pager::open(&path), Err(Error::Busy)));
    drop(db);
    Pager::open(&path).unwrap();
}

#[test]
fn truncated_and_corrupted_files_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.emily");
    drop(Pager::create(&path).unwrap());
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(100)
        .unwrap();
    assert!(matches!(Pager::open(&path), Err(Error::FileLength(100))));
    fs::remove_file(&path).unwrap();
    let mut db = Pager::create(&path).unwrap();
    db.write_page(&Page::new(1).unwrap()).unwrap();
    drop(db);
    let mut file = OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(PAGE_SIZE as u64 + 100)).unwrap();
    file.write_all(&[1]).unwrap();
    file.sync_all().unwrap();
    let mut db = Pager::open(&path).unwrap();
    assert!(matches!(db.verify(), Err(Error::Checksum)));
}

#[test]
fn page_addresses_are_bounded_and_gaps_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Pager::create(dir.path().join("bounds.emily")).unwrap();
    assert!(matches!(db.read_page(0), Err(Error::PageId(0))));
    assert!(matches!(db.read_page(u64::MAX), Err(Error::PageId(_))));
    assert!(db.write_page(&Page::new(2).unwrap()).is_err());
    assert_eq!(db.page_count(), 0);
}

#[test]
fn simultaneous_creation_has_exactly_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("race.emily");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Pager::create(path).is_ok()
            })
        })
        .collect();
    let winners = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    Pager::open(&path).unwrap().verify().unwrap();
}
