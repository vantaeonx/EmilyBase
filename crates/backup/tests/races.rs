use std::sync::{Arc, Barrier};

use emilybase_backup::{create, inspect, restore};
use emilybase_transactions::Database;

#[test]
fn simultaneous_backups_to_one_path_publish_exactly_one_complete_archive() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("winner.backup");
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for index in 0..2 {
        let source = dir.path().join(format!("source-{index}"));
        drop(Database::create(&source).unwrap());
        let target = target.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let mut db = Database::open(source).unwrap();
            barrier.wait();
            create(&mut db, target)
        }));
    }
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        &inspect(target).unwrap(),
        results
            .iter()
            .find_map(|result| result.as_ref().ok())
            .unwrap()
    );
    assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emilybase-backup-")
    }));
}

#[test]
fn simultaneous_restores_to_one_path_publish_exactly_one_complete_directory() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("restored");
    let backup = dir.path().join("snapshot.backup");
    let mut db = Database::create(dir.path().join("source")).unwrap();
    let expected = create(&mut db, &backup).unwrap();
    drop(db);
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let target = target.clone();
        let backup = backup.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            restore(backup, target)
        }));
    }
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|result| result.as_ref().ok())
            .unwrap(),
        &expected
    );
    let restored = Database::open(target).unwrap();
    assert_eq!(restored.database_id(), expected.database_id);
    assert_eq!(restored.last_transaction(), expected.last_transaction);
    assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".emilybase-backup-")
    }));
}
