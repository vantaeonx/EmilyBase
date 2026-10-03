use emilybase_transactions::{Database, Error};
use emilybase_wal::Wal;

#[test]
fn stable_directory_owner_excludes_reopen_after_wal_inode_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let owner = Database::create(&path).unwrap();
    let id = owner.database_id();
    let replacement = path.join("replacement.wal");
    let pages = owner.view().unwrap().pages().cloned().collect::<Vec<_>>();
    drop(Wal::create_snapshot(&replacement, id, 1, &pages).unwrap());
    std::fs::rename(replacement, path.join("redo.wal")).unwrap();
    assert!(matches!(
        Database::open(&path),
        Err(Error::Wal(emilybase_wal::Error::Busy))
    ));
    drop(owner);
    let reopened = Database::open_bound(path, Some(id)).unwrap();
    assert_eq!(reopened.last_transaction(), 1);
    assert_eq!(reopened.view().unwrap().row_count(), 0);
}
