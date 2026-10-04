#![no_main]
#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;

use emilybase_storage::Page;
use emilybase_wal::{Error, Recovery, Wal, encode_header, encode_snapshot, recover};
use libfuzzer_sys::fuzz_target;
mod support;

fn offered(path: &std::path::Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

fn compare(actual: &Recovery, expected: &Recovery) {
    assert_eq!(actual.database_id, expected.database_id);
    assert_eq!(actual.format_version, expected.format_version);
    assert_eq!(actual.valid_bytes, expected.valid_bytes);
    assert_eq!(actual.discarded_bytes, expected.discarded_bytes);
    assert_eq!(actual.last_transaction(), expected.last_transaction());
    match (&actual.baseline, &expected.baseline) {
        (Some(a), Some(b)) => {
            assert_eq!(a.transaction, b.transaction);
            assert_eq!(a.pages, b.pages);
        }
        (None, None) => (),
        _ => panic!("baseline presence differs"),
    }
    assert_eq!(actual.committed.len(), expected.committed.len());
    for (a, b) in actual.committed.iter().zip(&expected.committed) {
        assert_eq!(a.transaction, b.transaction);
        assert_eq!(a.pages, b.pages);
    }
}

fuzz_target!(|input: &[u8]| {
    // Keep filesystem work, page payloads and differential decoding bounded.
    if !(4..=20000).contains(&input.len()) {
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("journal");
    let moved = temporary.path().join("moved");
    let alias = temporary.path().join("alias");
    let mut page = Page::new(1).unwrap();
    page.insert(&input[4..input.len().min(260)]).unwrap();
    let pages = [page];
    let mut bytes = match input[0] % 4 {
        0 => input[4..].to_vec(),
        1 => {
            let mut image = encode_header([7; 16]).unwrap().to_vec();
            image.extend_from_slice(&input[4..]);
            image
        }
        2 => support::repaired_wal(&input[4..]).unwrap_or_else(|| input[4..].to_vec()),
        _ => {
            if input[0] & 4 == 0 {
                // Exercise original owned initialization and version-1 commits too.
                fs::write(&path, []).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                let mut wal = Wal::create_from_file(offered(&path), [7; 16]).unwrap();
                assert_eq!(wal.append(&pages).unwrap(), 1);
                wal.committed_bytes().unwrap()
            } else {
                encode_snapshot([7; 16], 31, &pages).unwrap()
            }
        }
    };
    // Valid seeds reach recovery, mutations and byte cuts reach strict admission.
    if input[3] & 1 != 0 && !bytes.is_empty() {
        let offset = (usize::from(input[1]) * 257 + usize::from(input[2])) % bytes.len();
        if input[3] & 2 == 0 {
            bytes[offset] ^= input[3];
        } else {
            bytes.truncate(offset);
        }
    }
    fs::write(&path, &bytes).unwrap();
    // Existing ordinary modes deliberately remain admissible on open.
    let mode = if input[2] & 4 == 0 { 0o600 } else { 0o644 };
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    let mut file = offered(&path);
    file.seek(SeekFrom::Start(u64::from(input[2]) * 257))
        .unwrap();
    let namespace = input[1] % 5;
    let mut busy_owner = None;
    let selected = match namespace {
        1 => {
            fs::hard_link(&path, &alias).unwrap();
            assert!(matches!(Wal::open(&alias, None), Err(Error::Path)));
            path.as_path()
        }
        2 => {
            fs::rename(&path, &moved).unwrap();
            fs::write(&path, b"foreign original name").unwrap();
            moved.as_path()
        }
        3 => {
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            assert!(Wal::open(&alias, None).is_err());
            path.as_path()
        }
        4 => {
            let owner = offered(&path);
            owner.try_lock().unwrap();
            busy_owner = Some(owner);
            path.as_path()
        }
        _ => path.as_path(),
    };
    let expected_id = match input[2] % 3 {
        0 => None,
        1 => Some([7; 16]),
        _ => Some([9; 16]),
    };
    let actual = Wal::open_from_file(file, expected_id);
    // Opening and every refusal preserve the whole input, including abandoned tails.
    assert_eq!(fs::read(selected).unwrap(), bytes);
    if namespace == 2 {
        assert_eq!(fs::read(&path).unwrap(), b"foreign original name");
    }
    if namespace == 1 {
        assert!(matches!(actual, Err(Error::Path)));
        assert_eq!(fs::read(&alias).unwrap(), bytes);
        return;
    }
    if namespace == 4 {
        assert!(matches!(actual, Err(Error::Busy)));
        drop(busy_owner);
        return;
    }
    match (actual, recover(&bytes, expected_id)) {
        (Ok((mut wal, actual)), Ok(expected)) => {
            compare(&actual, &expected);
            assert_eq!(wal.valid_bytes(), expected.valid_bytes as u64);
            assert_eq!(
                wal.committed_bytes().unwrap(),
                bytes[..expected.valid_bytes]
            );
            assert!(matches!(Wal::open(selected, None), Err(Error::Busy)));
            let next = expected.last_transaction() + 1;
            assert_eq!(wal.append(&pages).unwrap(), next);
            let written = wal.committed_bytes().unwrap();
            assert_eq!(
                &written[..expected.valid_bytes],
                &bytes[..expected.valid_bytes]
            );
            let after = recover(&written, expected_id).unwrap();
            assert_eq!(after.last_transaction(), next);
            assert_eq!(after.discarded_bytes, 0);
            assert_eq!(after.committed.last().unwrap().pages, pages);
            drop(wal);
            let (_, reopened) = Wal::open(selected, expected_id).unwrap();
            compare(&reopened, &after);
        }
        (Err(actual), Err(expected)) => {
            assert_eq!(
                std::mem::discriminant(&actual),
                std::mem::discriminant(&expected)
            );
            // A failed decoder must release the exclusive descriptor lock too.
            let probe = offered(selected);
            probe.try_lock().unwrap();
        }
        _ => panic!("owned-file recovery differs from byte decoder"),
    }
});
