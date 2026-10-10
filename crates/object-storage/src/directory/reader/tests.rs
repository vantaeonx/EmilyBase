use super::*;
use proptest::prelude::*;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn setup(parent: &Path, payload: &[u8]) -> ProjectDirectory {
    let path = parent.join("objects");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let mut owner = ProjectDirectory::initialize(path, PROJECT).unwrap();
    owner.put(OBJECT, payload).unwrap();
    owner
}
fn object_path(parent: &Path) -> std::path::PathBuf {
    parent.join("objects").join(object_name(OBJECT))
}

#[test]
fn reads_only_payload_keeps_actual_descriptor_and_native_owner_without_payload_image() {
    let temp = tempfile::tempdir().unwrap();
    let payload = b"synthetic-secret\0\xff";
    let owner = setup(temp.path(), payload);
    let expected = owner.inspect(OBJECT).unwrap();
    let mut reader = owner.reader(OBJECT).unwrap();
    assert_eq!(reader.project(), PROJECT);
    assert_eq!(reader.object(), OBJECT);
    assert_eq!(reader.report(), &expected);
    assert_eq!(reader.payload_position(), 0);
    assert_eq!(
        reader.file.metadata().unwrap().ino(),
        fs::metadata(object_path(temp.path())).unwrap().ino()
    );
    assert!(std::mem::size_of::<ObjectReader<'_>>() < 1024);
    assert!(matches!(
        ProjectDirectory::open(temp.path().join("objects"), PROJECT),
        Err(Error::Busy)
    ));
    let mut buffer = [0x77; 64];
    let count = reader.read_payload(&mut buffer).unwrap();
    assert_eq!(count, payload.len());
    assert_eq!(&buffer[..count], payload);
    assert_eq!(&buffer[count..], &[0x77; 64][count..]);
    assert_eq!(reader.payload_position(), payload.len() as u64);
    assert!(!format!("{reader:?}").contains("synthetic-secret"));
    assert_eq!(reader.read_payload(&mut buffer).unwrap(), 0);
    assert_eq!(reader.finish().unwrap(), expected);
    assert_eq!(owner.get(OBJECT).unwrap().payload(), payload);
    drop(owner);
    ProjectDirectory::open(temp.path().join("objects"), PROJECT).unwrap();
}

#[test]
fn maximum_and_empty_readonly_objects_respect_chunk_eof_and_exact_original_hash() {
    for length in [0, 1, MAX_OBJECT_READ_BYTES, MAX_PAYLOAD_BYTES] {
        let temp = tempfile::tempdir().unwrap();
        let payload: Vec<_> = (0..length).map(|index| (index % 251) as u8).collect();
        let owner = setup(temp.path(), &payload);
        fs::set_permissions(object_path(temp.path()), fs::Permissions::from_mode(0o400)).unwrap();
        let mut reader = owner.reader(OBJECT).unwrap();
        let expected = reader.report().clone();
        let mut offset = 0;
        let mut buffer = vec![0x77; 3 * MAX_OBJECT_READ_BYTES];
        loop {
            buffer.fill(0x77);
            let count = reader.read_payload(&mut buffer).unwrap();
            assert!(count <= MAX_OBJECT_READ_BYTES);
            assert_eq!(&buffer[..count], &payload[offset..offset + count]);
            assert!(buffer[count..].iter().all(|byte| *byte == 0x77));
            offset += count;
            if count == 0 {
                break;
            }
        }
        assert_eq!(offset, length);
        assert_eq!(reader.finish().unwrap(), expected);
    }
}

#[test]
fn payload_seek_is_checked_header_relative_and_independent_of_full_hash_file_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let owner = setup(temp.path(), b"0123456789");
    let mut reader = owner.reader(OBJECT).unwrap();
    assert_eq!(reader.seek_payload(SeekFrom::End(-3)).unwrap(), 7);
    reader.verify().unwrap();
    assert_eq!(reader.payload_position(), 7);
    let mut buffer = [0; 2];
    assert_eq!(reader.read_payload(&mut buffer).unwrap(), 2);
    assert_eq!(&buffer, b"78");
    assert_eq!(reader.seek_payload(SeekFrom::Current(-8)).unwrap(), 1);
    assert_eq!(reader.read_payload(&mut buffer).unwrap(), 2);
    assert_eq!(&buffer, b"12");
    for from in [SeekFrom::Current(-4), SeekFrom::End(-11)] {
        assert!(matches!(reader.seek_payload(from), Err(Error::InvalidSeek)));
        assert_eq!(reader.payload_position(), 3);
        reader.verify().unwrap();
    }
    assert_eq!(
        reader.seek_payload(SeekFrom::Start(u64::MAX)).unwrap(),
        u64::MAX
    );
    assert_eq!(reader.read_payload(&mut buffer).unwrap(), 0);
    assert!(matches!(
        reader.seek_payload(SeekFrom::Current(1)),
        Err(Error::InvalidSeek)
    ));
    assert_eq!(reader.payload_position(), u64::MAX);
    assert_eq!(
        reader.seek_payload(SeekFrom::End(i64::MAX)).unwrap(),
        10 + i64::MAX as u64
    );
    assert_eq!(reader.seek_payload(SeekFrom::Start(0)).unwrap(), 0);
    assert_eq!(reader.read_payload(&mut buffer).unwrap(), 2);
    assert_eq!(&buffer, b"01");
    reader.finish().unwrap();
}

fn mutate(parent: &Path, shape: u8) {
    let path = object_path(parent);
    match shape {
        0 => fs::write(path, b"broken").unwrap(),
        1 => fs::write(path, encode(PROJECT, OBJECT, b"different").unwrap()).unwrap(),
        2 => fs::write(
            path,
            encode(ProjectId::from_bytes([3; 16]), OBJECT, b"synthetic").unwrap(),
        )
        .unwrap(),
        3 => fs::write(
            path,
            encode(PROJECT, ObjectId::from_bytes([3; 16]), b"synthetic").unwrap(),
        )
        .unwrap(),
        4 => File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(96)
            .unwrap(),
        5 => File::options()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b"x")
            .unwrap(),
        6 => fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap(),
        7 => fs::hard_link(path, parent.join("linked")).unwrap(),
        8 | 9 => {
            let saved = parent.join("original");
            fs::rename(&path, &saved).unwrap();
            if shape == 8 {
                symlink(saved, path).unwrap();
            } else {
                fs::copy(saved, path).unwrap();
            }
        }
        10 => {
            let marker = parent.join("objects").join(SCOPE_FILE);
            fs::rename(&marker, parent.join("original-marker")).unwrap();
            fs::write(marker, encode(PROJECT, SCOPE_OBJECT, b"").unwrap()).unwrap();
        }
        11 => fs::write(parent.join("objects").join(SCOPE_FILE), b"broken marker").unwrap(),
        12 => {
            fs::set_permissions(parent.join("objects"), fs::Permissions::from_mode(0o755)).unwrap()
        }
        _ => unreachable!("test mutation shape"),
    }
}
fn restore_test_permissions(parent: &Path) {
    fs::set_permissions(parent.join("objects"), fs::Permissions::from_mode(0o700)).unwrap();
}
fn assert_poisoned(reader: &mut ObjectReader<'_>) {
    let position = reader.payload_position();
    assert!(matches!(
        reader.read_payload(&mut []),
        Err(Error::ReaderPoisoned)
    ));
    assert!(matches!(
        reader.seek_payload(SeekFrom::Start(0)),
        Err(Error::ReaderPoisoned)
    ));
    assert!(matches!(reader.verify(), Err(Error::ReaderPoisoned)));
    assert_eq!(reader.payload_position(), position);
}

#[test]
fn read_mutations_before_and_after_copy_refuse_clear_attempted_bytes_and_keep_original_inode() {
    for after_copy in [false, true] {
        for shape in 0..13 {
            let temp = tempfile::tempdir().unwrap();
            let owner = setup(temp.path(), b"synthetic");
            let mut reader = owner.reader(OBJECT).unwrap();
            let inode = reader.file.metadata().unwrap().ino();
            let mut destination = [0x77; 32];
            if !after_copy {
                mutate(temp.path(), shape);
            }
            let result = reader.read_payload_with(&mut destination, || {
                if after_copy {
                    mutate(temp.path(), shape);
                }
            });
            assert!(result.is_err(), "shape {shape}, after_copy {after_copy}");
            assert_eq!(reader.payload_position(), 0);
            assert_eq!(&destination[..9], &[0; 9]);
            assert_eq!(&destination[9..], &[0x77; 23]);
            assert_eq!(reader.file.metadata().unwrap().ino(), inode);
            assert_poisoned(&mut reader);
            assert!(object_path(temp.path()).symlink_metadata().is_ok());
            restore_test_permissions(temp.path());
        }
    }
}

#[test]
fn full_verification_and_final_result_refuse_late_mutations_without_refreshing_expected_report() {
    for shape in 0..13 {
        let temp = tempfile::tempdir().unwrap();
        let owner = setup(temp.path(), b"synthetic");
        let mut reader = owner.reader(OBJECT).unwrap();
        reader.seek_payload(SeekFrom::Start(4)).unwrap();
        let expected = reader.report().clone();
        assert!(
            reader.verify_with(|| mutate(temp.path(), shape)).is_err(),
            "shape {shape}"
        );
        assert_eq!(reader.payload_position(), 4);
        assert_eq!(reader.report(), &expected);
        assert_poisoned(&mut reader);
        assert!(matches!(reader.finish(), Err(Error::ReaderPoisoned)));
        restore_test_permissions(temp.path());
    }
}

#[test]
fn admission_final_check_refuses_same_bytes_new_inode_and_late_scope_or_metadata_mutations() {
    for shape in 0..13 {
        let temp = tempfile::tempdir().unwrap();
        let owner = setup(temp.path(), b"synthetic");
        let expected = owner.inspect(OBJECT).unwrap();
        assert!(
            owner
                .reader_with(OBJECT, || mutate(temp.path(), shape))
                .is_err(),
            "shape {shape}"
        );
        if shape == 9 {
            assert_eq!(owner.inspect(OBJECT).unwrap(), expected);
        }
        restore_test_permissions(temp.path());
    }
}

#[test]
fn initial_admission_refuses_missing_foreign_corrupt_and_nonregular_objects_without_repairs() {
    let temp = tempfile::tempdir().unwrap();
    let owner = setup(temp.path(), b"synthetic");
    assert!(owner.reader(ObjectId::from_bytes([9; 16])).is_err());
    assert!(matches!(
        ProjectDirectory::open(temp.path().join("objects"), ProjectId::from_bytes([9; 16])),
        Err(Error::Busy)
    ));
    drop(owner);
    assert!(
        ProjectDirectory::open(temp.path().join("objects"), ProjectId::from_bytes([9; 16]))
            .is_err()
    );
    for shape in [0, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12] {
        let temp = tempfile::tempdir().unwrap();
        let owner = setup(temp.path(), b"synthetic");
        mutate(temp.path(), shape);
        assert!(owner.reader(OBJECT).is_err(), "shape {shape}");
        restore_test_permissions(temp.path());
    }
    for fifo in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let owner = setup(temp.path(), b"synthetic");
        let path = object_path(temp.path());
        fs::remove_file(&path).unwrap();
        if fifo {
            rustix::fs::mknodat(
                rustix::fs::CWD,
                &path,
                rustix::fs::FileType::Fifo,
                Mode::RUSR | Mode::WUSR,
                0,
            )
            .unwrap();
        } else {
            fs::create_dir(&path).unwrap();
        }
        assert!(owner.reader(OBJECT).is_err());
        assert!(path.exists());
    }
}

#[test]
fn empty_eof_and_seek_checks_cannot_bypass_scope_and_poison_is_irreversible_after_repair() {
    for operation in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let owner = setup(temp.path(), b"synthetic");
        let mut reader = owner.reader(OBJECT).unwrap();
        reader.seek_payload(SeekFrom::End(0)).unwrap();
        let marker = temp.path().join("objects").join(SCOPE_FILE);
        let original = fs::read(&marker).unwrap();
        fs::write(&marker, b"damaged").unwrap();
        match operation {
            0 => assert!(reader.read_payload(&mut []).is_err()),
            1 => assert!(reader.read_payload(&mut [0x77; 8]).is_err()),
            _ => assert!(reader.seek_payload(SeekFrom::Start(0)).is_err()),
        }
        fs::write(marker, original).unwrap();
        assert_poisoned(&mut reader);
        assert_eq!(
            owner
                .reader(OBJECT)
                .unwrap()
                .finish()
                .unwrap()
                .payload_bytes,
            9
        );
    }
}

#[test]
fn moved_directory_keeps_original_namespace_and_independent_reader_cursors() {
    let temp = tempfile::tempdir().unwrap();
    let owner = setup(temp.path(), b"synthetic");
    let mut first = owner.reader(OBJECT).unwrap();
    let mut second = owner.reader(OBJECT).unwrap();
    fs::rename(temp.path().join("objects"), temp.path().join("moved")).unwrap();
    let replacement = setup(temp.path(), b"different");
    first.seek_payload(SeekFrom::End(-3)).unwrap();
    let mut buffer = [0; 3];
    assert_eq!(first.read_payload(&mut buffer).unwrap(), 3);
    assert_eq!(&buffer, b"tic");
    assert_eq!(second.read_payload(&mut buffer).unwrap(), 3);
    assert_eq!(&buffer, b"syn");
    assert_eq!(second.payload_position(), 3);
    assert_eq!(first.payload_position(), 9);
    assert_eq!(replacement.get(OBJECT).unwrap().payload(), b"different");
    first.finish().unwrap();
    second.finish().unwrap();
}

#[test]
fn concurrent_scoped_readers_keep_independent_positions_and_exclude_a_second_owner() {
    let temp = tempfile::tempdir().unwrap();
    let payload: Vec<_> = (0..24_000).map(|index| (index % 251) as u8).collect();
    let owner = setup(temp.path(), &payload);
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for worker in 0..6 {
            let owner = &owner;
            let payload = &payload;
            workers.push(scope.spawn(move || {
                let mut reader = owner.reader(OBJECT).unwrap();
                let mut buffer = [0; 257];
                for step in 0..24 {
                    let position = (worker * 271 + step * 769) % (payload.len() - buffer.len());
                    reader
                        .seek_payload(SeekFrom::Start(position as u64))
                        .unwrap();
                    assert_eq!(reader.read_payload(&mut buffer).unwrap(), buffer.len());
                    assert_eq!(&buffer, &payload[position..position + buffer.len()]);
                    reader.verify().unwrap();
                    assert_eq!(reader.payload_position(), (position + buffer.len()) as u64);
                }
                reader.finish().unwrap()
            }));
        }
        assert!(matches!(
            ProjectDirectory::open(temp.path().join("objects"), PROJECT),
            Err(Error::Busy)
        ));
        for worker in workers {
            assert_eq!(worker.join().unwrap(), owner.inspect(OBJECT).unwrap());
        }
    });
    assert_eq!(owner.get(OBJECT).unwrap().payload(), payload);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_binary_payload_cursor_matches_independent_slice_model(
        payload in prop::collection::vec(any::<u8>(), 0..32_769),
        operations in prop::collection::vec((0u8..5, any::<u64>(), any::<i64>(), 0usize..20_000), 1..48),
    ) {
        let temp = tempfile::tempdir().unwrap();
        let owner = setup(temp.path(), &payload);
        let mut reader = owner.reader(OBJECT).unwrap();
        let mut position = 0u64;
        for (kind, absolute, delta, size) in operations {
            if kind < 3 {
                let (from, expected) = match kind {
                    0 => (SeekFrom::Start(absolute), Some(absolute)),
                    1 => (SeekFrom::Current(delta), position.checked_add_signed(delta)),
                    _ => (SeekFrom::End(delta), (payload.len() as u64).checked_add_signed(delta)),
                };
                match expected {
                    Some(next) => {
                        prop_assert_eq!(reader.seek_payload(from).unwrap(), next);
                        position = next;
                    }
                    None => prop_assert!(matches!(reader.seek_payload(from), Err(Error::InvalidSeek))),
                }
            } else if kind == 3 {
                let mut buffer = vec![0x77; size];
                let expected = (payload.len() as u64).saturating_sub(position)
                    .min(size as u64).min(MAX_OBJECT_READ_BYTES as u64) as usize;
                let count = reader.read_payload(&mut buffer).unwrap();
                prop_assert_eq!(count, expected);
                if expected > 0 {
                    prop_assert_eq!(&buffer[..count], &payload[position as usize..position as usize + count]);
                }
                prop_assert!(buffer[count..].iter().all(|byte| *byte == 0x77));
                position += count as u64;
            } else {
                reader.verify().unwrap();
            }
            prop_assert_eq!(reader.payload_position(), position);
        }
        prop_assert_eq!(reader.finish().unwrap(), owner.inspect(OBJECT).unwrap());
    }
}
