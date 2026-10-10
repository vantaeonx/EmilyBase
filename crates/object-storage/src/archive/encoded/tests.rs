use super::*;
use crate::archive::{encode_archive, encode_checked, verify_archive, verify_archive_reader};
use crate::directory::inventory::digest_components;
use crate::{ProjectDirectory, encode};
use proptest::prelude::*;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::DirBuilderExt;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);

fn canonical(values: &[Vec<u8>]) -> Vec<u8> {
    let ids: Vec<_> = (0..values.len())
        .map(|i| {
            let mut id = [0; 16];
            id[15] = i as u8;
            ObjectId::from_bytes(id)
        })
        .collect();
    let total = values.iter().map(|p| p.len() as u64).sum();
    let digest = digest_components(
        PROJECT,
        values.len() as u32,
        total,
        ids.iter()
            .zip(values)
            .map(|(id, p)| (*id, p.len() as u64, Sha256::digest(p).into())),
    );
    let images: Vec<_> = ids
        .iter()
        .zip(values)
        .map(|(id, p)| encode(PROJECT, *id, p).unwrap())
        .collect();
    encode_checked(
        PROJECT,
        total,
        digest,
        ids.iter()
            .zip(&images)
            .map(|(id, bytes)| (*id, bytes.as_slice())),
    )
    .unwrap()
}

fn compare_chunks(reader: &mut ArchiveReader<'_>, expected: &[u8], chunk: usize) {
    reader.rewind().unwrap();
    let mut buffer = vec![0xaa; chunk];
    let mut offset = 0;
    loop {
        let count = reader.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        assert_eq!(&buffer[..count], &expected[offset..offset + count]);
        offset += count;
        buffer.fill(0xaa);
    }
    assert_eq!(offset, expected.len());
    assert_eq!(buffer, vec![0xaa; chunk]);
    assert_eq!(reader.stream_position().unwrap(), expected.len() as u64);
}

#[test]
fn independent_known_bytes_remain_canonical_without_payload_copies() {
    // Independent Python struct/hashlib/zlib vector, shared with format acceptance.
    let hex = "454d494c594f424b010000008000000001010101010101010101010101010101010000000000000013000000000000008b0000000000000074bf5d494d0be62eef120a24ce5413aaa7c1cae7b821084ba746d0191f11a0dc4031e0807d7c3fc9ab2aa3fc2851b922ccf4bfdc15e5dbf49b027df30f6be720000000007a1c3073020202020202020202020202020202027300000000000000454d494c594f424a010000006000000001010101010101010101010101010101020202020202020202020202020202021300000000000000ea0d8b4b4bc2442d1dd5b10fe97ebbcb7a23f4bb3da838fbcf588c625caef63300000000691e2ec573796e7468657469632d6172636869766500ff";
    let bytes: Vec<_> = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    let view = verify_archive(&bytes, PROJECT).unwrap();
    let mut reader = ArchiveReader::from_verified(&view).unwrap();
    assert_eq!(reader.encoded_bytes(), bytes.len());
    assert_eq!(reader.report().objects, 1);
    assert_eq!(reader.report().payload_bytes, 19);
    assert_eq!(&reader.report().digest, view.digest());
    assert_eq!(
        reader.parts[0].image.as_ptr(),
        view.objects()[0].image.as_ptr()
    );
    for chunk in [1, 7, 24, 96, 127, 128, 8192] {
        compare_chunks(&mut reader, &bytes, chunk);
    }
    assert_eq!(
        verify_archive_reader(&mut reader, bytes.len(), PROJECT).unwrap(),
        *reader.report()
    );
    assert!(!format!("{reader:?}").contains("synthetic-archive"));
}

#[test]
fn captured_bytes_stay_owned_and_borrowed_after_original_namespace_is_removed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    owner.put(OBJECT, b"synthetic-snapshot").unwrap();
    owner.put(ObjectId::from_bytes([4; 16]), b"").unwrap();
    let snapshot = owner.capture().unwrap();
    let expected = encode_archive(&snapshot).unwrap();
    drop(owner);
    fs::remove_dir_all(&path).unwrap();
    let mut reader = ArchiveReader::from_snapshot(&snapshot).unwrap();
    for (part, object) in reader.parts.iter().zip(snapshot.objects()) {
        assert_eq!(part.image.as_ptr(), object.encoded().as_ptr());
    }
    compare_chunks(&mut reader, &expected, 8192);
    assert_eq!(reader.report().digest, *snapshot.inventory().digest());
}

#[test]
fn every_segment_boundary_empty_read_and_repeated_eof_match_a_byte_cursor() {
    let bytes = canonical(&[vec![], vec![0, 255], vec![], vec![31; 9000]]);
    let view = verify_archive(&bytes, PROJECT).unwrap();
    let mut reader = ArchiveReader::from_verified(&view).unwrap();
    let mut positions = vec![0, 127, 128, bytes.len() - 1, bytes.len(), bytes.len() + 1];
    for part in &reader.parts {
        let start = part.start as usize;
        positions.extend([
            start - 1,
            start,
            start + 23,
            start + 24,
            start + 24 + HEADER_BYTES - 1,
            start + 24 + part.image.len(),
        ]);
    }
    for position in positions {
        let mut cursor = Cursor::new(&bytes);
        cursor.set_position(position as u64);
        reader.seek(SeekFrom::Start(position as u64)).unwrap();
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        assert_eq!(reader.stream_position().unwrap(), position as u64);
        let mut a = [0xab; 257];
        let mut b = a;
        assert_eq!(reader.read(&mut a).unwrap(), cursor.read(&mut b).unwrap());
        assert_eq!(a, b);
        assert_eq!(reader.stream_position().unwrap(), cursor.position());
    }
    reader.seek(SeekFrom::End(0)).unwrap();
    for _ in 0..3 {
        assert_eq!(reader.read(&mut [0; 5]).unwrap(), 0);
    }
}

#[test]
fn seek_arithmetic_refuses_negative_and_overflow_without_changing_position() {
    let bytes = canonical(&[]);
    let view = verify_archive(&bytes, PROJECT).unwrap();
    let mut reader = ArchiveReader::from_verified(&view).unwrap();
    assert_eq!(reader.seek(SeekFrom::End(-1)).unwrap(), 127);
    assert_eq!(reader.seek(SeekFrom::Current(-127)).unwrap(), 0);
    for from in [
        SeekFrom::Current(-1),
        SeekFrom::Current(i64::MIN),
        SeekFrom::End(-129),
        SeekFrom::End(i64::MIN),
    ] {
        assert_eq!(
            reader.seek(from).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(reader.stream_position().unwrap(), 0);
    }
    reader.seek(SeekFrom::Start(u64::MAX)).unwrap();
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    assert_eq!(
        reader.seek(SeekFrom::Current(1)).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(reader.stream_position().unwrap(), u64::MAX);
    assert_eq!(
        reader.seek(SeekFrom::Current(i64::MIN)).unwrap(),
        i64::MAX as u64
    );
    assert_eq!(
        reader.seek(SeekFrom::End(i64::MAX)).unwrap(),
        128 + i64::MAX as u64
    );
    reader.rewind().unwrap();
    compare_chunks(&mut reader, &bytes, 23);
}

#[test]
fn exact_empty_and_maximum_archive_shapes_match_without_read_image_allocation() {
    for values in [
        vec![],
        vec![vec![]; MAX_INVENTORY_OBJECTS],
        vec![
            vec![0x91; MAX_INVENTORY_BYTES as usize / MAX_INVENTORY_OBJECTS];
            MAX_INVENTORY_OBJECTS
        ],
    ] {
        let bytes = canonical(&values);
        drop(values);
        let view = verify_archive(&bytes, PROJECT).unwrap();
        let mut reader = ArchiveReader::from_verified(&view).unwrap();
        assert_eq!(reader.parts.len(), view.objects().len());
        assert!(std::mem::size_of::<Part<'_>>() <= 64);
        compare_chunks(&mut reader, &bytes, 8192);
        assert_eq!(
            verify_archive_reader(&mut reader, bytes.len(), PROJECT).unwrap(),
            *reader.report()
        );
        if view.payload_bytes() == MAX_INVENTORY_BYTES {
            assert_eq!(reader.encoded_bytes(), crate::MAX_ARCHIVE_BYTES);
        }
    }
}

#[test]
fn private_constructor_rejects_inconsistent_metadata_before_exposing_a_reader() {
    let nested = encode(PROJECT, OBJECT, b"abc").unwrap();
    fn make(total: u64, entries: Vec<(ObjectId, &[u8])>) -> Result<ArchiveReader<'_>> {
        ArchiveReader::new(PROJECT, total, [0; 32], entries.into_iter())
    }
    assert!(matches!(
        make(MAX_INVENTORY_BYTES + 1, vec![]),
        Err(Error::Limit)
    ));
    assert!(matches!(
        make(0, vec![(OBJECT, &nested); 129]),
        Err(Error::Limit)
    ));
    assert!(matches!(
        make(0, vec![(OBJECT, &nested)]),
        Err(Error::Archive)
    ));
    assert!(matches!(
        make(4, vec![(OBJECT, &nested)]),
        Err(Error::Archive)
    ));
    assert!(matches!(
        make(0, vec![(OBJECT, &[0; 95])]),
        Err(Error::Archive)
    ));
    assert!(matches!(
        make(6, vec![(OBJECT, &nested), (OBJECT, &nested)]),
        Err(Error::Archive)
    ));
    let large = vec![0; MAX_PAYLOAD_BYTES + HEADER_BYTES + 1];
    assert!(matches!(make(0, vec![(OBJECT, &large)]), Err(Error::Limit)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_binary_archives_and_seek_read_sequences_match_cursor(
        values in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..2048), 0..12),
        operations in prop::collection::vec((any::<u8>(), any::<i16>(), 0usize..1024), 1..100),
    ) {
        let bytes = canonical(&values);
        let view = verify_archive(&bytes, PROJECT).unwrap();
        let mut reader = ArchiveReader::from_verified(&view).unwrap();
        let mut cursor = Cursor::new(&bytes);
        for (kind, offset, size) in operations {
            let from = match kind % 3 {
                0 => SeekFrom::Start(offset as u16 as u64),
                1 => SeekFrom::Current(offset as i64),
                _ => SeekFrom::End(offset as i64),
            };
            let a = reader.seek(from);
            let b = cursor.seek(from);
            match (a, b) {
                (Ok(a), Ok(b)) => prop_assert_eq!(a, b),
                (Err(a), Err(b)) => prop_assert_eq!(a.kind(), b.kind()),
                _ => prop_assert!(false, "seek admission differs"),
            }
            prop_assert_eq!(reader.stream_position().unwrap(), cursor.position());
            let mut a = vec![0xae; size];
            let mut b = a.clone();
            prop_assert_eq!(reader.read(&mut a).unwrap(), cursor.read(&mut b).unwrap());
            prop_assert_eq!(a, b);
        }
        compare_chunks(&mut reader, &bytes, 137);
    }
}
