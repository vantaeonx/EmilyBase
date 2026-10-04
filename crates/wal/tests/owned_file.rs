use emilybase_storage::Page;
use emilybase_wal::{Error, Wal, encode_snapshot};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

fn empty(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}
fn pages() -> Vec<Page> {
    (1..=2)
        .map(|id| {
            let mut page = Page::new(id).unwrap();
            page.insert(&[0, 255, id as u8]).unwrap();
            page
        })
        .collect()
}

#[test]
fn owned_baseline_binds_the_exact_descriptor_and_retains_lock_until_last_clone_closes() {
    let temporary = tempfile::tempdir().unwrap();
    let original = temporary.path().join("original");
    let renamed = temporary.path().join("renamed");
    let foreign = temporary.path().join("foreign");
    let mut file = empty(&original);
    let probe = file.try_clone().unwrap();
    file.seek(SeekFrom::Start(1234)).unwrap();
    fs::rename(&original, &renamed).unwrap();
    fs::write(&original, b"foreign original name").unwrap();
    let mut owner = Wal::create_snapshot_from_file(file, [7; 16], 31, &pages()).unwrap();
    let expected = encode_snapshot([7; 16], 31, &pages()).unwrap();
    assert_eq!(fs::read(&renamed).unwrap(), expected);
    assert_eq!(fs::read(&original).unwrap(), b"foreign original name");
    assert!(owner.owns_file(&probe).unwrap());
    assert!(owner.owns_file(&File::open(&renamed).unwrap()).unwrap());
    fs::write(&foreign, &expected).unwrap();
    assert!(!owner.owns_file(&File::open(foreign).unwrap()).unwrap());
    assert!(matches!(Wal::open(&renamed, None), Err(Error::Busy)));
    assert_eq!(owner.committed_bytes().unwrap(), expected);
    owner.begin(&pages()).unwrap().rollback().unwrap();
    assert_eq!(owner.append(&pages()).unwrap(), 32);
    drop(owner);
    assert!(matches!(Wal::open(&renamed, None), Err(Error::Busy)));
    drop(probe);
    let (mut reopened, recovery) = Wal::open(renamed, Some([7; 16])).unwrap();
    assert_eq!(recovery.last_transaction(), 32);
    assert_eq!(reopened.append(&pages()).unwrap(), 33);
}

#[test]
fn baseline_admission_errors_do_not_truncate_contents_links_or_empty_sources() {
    let temporary = tempfile::tempdir().unwrap();
    for case in 0..5 {
        let path = temporary.path().join(format!("source-{case}"));
        let file = empty(&path);
        let alias = temporary.path().join(format!("alias-{case}"));
        match case {
            0 => fs::write(&path, b"preserved nonempty file").unwrap(),
            1 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            2 => fs::hard_link(&path, &alias).unwrap(),
            3 => fs::remove_file(&path).unwrap(),
            _ => (),
        }
        let before = fs::read(&path).unwrap_or_default();
        let offered = if case == 4 {
            drop(file);
            File::open(&path).unwrap()
        } else {
            file
        };
        assert!(Wal::create_snapshot_from_file(offered, [7; 16], 1, &pages()).is_err());
        if case != 3 {
            assert_eq!(fs::read(&path).unwrap(), before);
        } else {
            assert!(!path.exists());
        }
        if case == 2 {
            assert_eq!(fs::read(alias).unwrap(), before);
        }
    }
    assert!(
        Wal::create_snapshot_from_file(File::open(temporary.path()).unwrap(), [7; 16], 1, &pages())
            .is_err()
    );
    for (number, id, transaction, initial) in [
        (0, [0; 16], 1, pages()),
        (1, [7; 16], 0, pages()),
        (2, [7; 16], u64::MAX, pages()),
        (3, [7; 16], 1, vec![Page::new(2).unwrap()]),
    ] {
        let path = temporary.path().join(format!("invalid-{number}"));
        assert!(Wal::create_snapshot_from_file(empty(&path), id, transaction, &initial).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
    }
}

#[test]
fn competing_empty_file_owner_is_refused_without_initializing_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("empty");
    let owner = empty(&path);
    owner.try_lock().unwrap();
    let separate = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    assert!(matches!(
        Wal::create_snapshot_from_file(separate, [7; 16], 1, &pages()),
        Err(Error::Busy)
    ));
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    drop(owner);
    let separate = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    drop(Wal::create_snapshot_from_file(separate, [7; 16], 1, &pages()).unwrap());
    assert_eq!(Wal::open(path, None).unwrap().1.last_transaction(), 1);
}

#[test]
fn version_one_owned_creation_and_both_version_recovery_start_at_offset_zero() {
    for baseline in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("wal");
        let mut file = empty(&path);
        file.seek(SeekFrom::Start(1234)).unwrap();
        let mut owner = if baseline {
            Wal::create_snapshot_from_file(file, [7; 16], 31, &pages()).unwrap()
        } else {
            Wal::create_from_file(file, [7; 16]).unwrap()
        };
        if !baseline {
            assert_eq!(
                fs::read(&path).unwrap(),
                emilybase_wal::encode_header([7; 16]).unwrap()
            );
            assert_eq!(owner.last_transaction(), 0);
            assert_eq!(owner.append(&pages()).unwrap(), 1);
        }
        let expected = owner.committed_bytes().unwrap();
        let transaction = owner.last_transaction();
        drop(owner);
        let mut offered = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        offered
            .seek(SeekFrom::Start(expected.len() as u64))
            .unwrap();
        let (mut reopened, recovery) = Wal::open_from_file(offered, Some([7; 16])).unwrap();
        assert_eq!(recovery.last_transaction(), transaction);
        assert_eq!(reopened.committed_bytes().unwrap(), expected);
        assert_eq!(reopened.append(&pages()).unwrap(), transaction + 1);
    }
}

#[test]
fn raw_journal_open_refuses_final_aliases_before_reading_or_mutating_the_source() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("wal");
    let mut owner = Wal::create(&path, [7; 16]).unwrap();
    owner.append(&pages()).unwrap();
    drop(owner);
    let before = fs::read(&path).unwrap();
    let alias = temporary.path().join("alias");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(Wal::open(&alias, None).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    assert!(matches!(Wal::open(&alias, None), Err(Error::Path)));
    assert!(matches!(Wal::open(&path, None), Err(Error::Path)));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&alias).unwrap(), before);
    fs::remove_file(alias).unwrap();
    assert_eq!(Wal::open(&path, None).unwrap().1.last_transaction(), 1);
}

#[test]
fn journal_type_size_identity_and_new_file_admission_fail_without_source_mutation() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("wal");
    let mut owner = Wal::create(&path, [7; 16]).unwrap();
    owner.append(&pages()).unwrap();
    drop(owner);
    let before = fs::read(&path).unwrap();
    let offered = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    assert!(matches!(
        Wal::open_from_file(offered, Some([9; 16])),
        Err(Error::Identity)
    ));
    let offered = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    assert!(Wal::create_from_file(offered, [7; 16]).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(matches!(
        Wal::open_from_file(File::open(temporary.path()).unwrap(), None),
        Err(Error::Path)
    ));
    assert!(matches!(
        Wal::open_from_file(File::open("/dev/null").unwrap(), None),
        Err(Error::Path)
    ));
    let fifo = temporary.path().join("fifo");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RWXU,
        0,
    )
    .unwrap();
    assert!(matches!(Wal::open(fifo, None), Err(Error::Path)));
    let oversized = temporary.path().join("oversized");
    let offered = empty(&oversized);
    offered
        .set_len(emilybase_wal::MAX_WAL_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(
        Wal::open_from_file(offered, None),
        Err(Error::Limit(_))
    ));
    assert_eq!(
        fs::metadata(oversized).unwrap().len(),
        emilybase_wal::MAX_WAL_BYTES as u64 + 1
    );
    let invalid = temporary.path().join("invalid-id");
    assert!(Wal::create_from_file(empty(&invalid), [0; 16]).is_err());
    assert_eq!(fs::metadata(invalid).unwrap().len(), 0);
}
