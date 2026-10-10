use super::*;
use crate::{ObjectId, encode, verify_archive};
use proptest::prelude::*;
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);

fn seal(bytes: &mut [u8]) {
    let body_hash = Sha256::digest(&bytes[128..]);
    bytes[88..120].copy_from_slice(&body_hash);
    header_crc(bytes);
}
fn header_crc(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}
fn image(values: &[(ObjectId, Vec<u8>)]) -> Vec<u8> {
    let total: u64 = values.iter().map(|(_, p)| p.len() as u64).sum();
    let digest = digest_components(
        PROJECT,
        values.len() as u32,
        total,
        values
            .iter()
            .map(|(id, p)| (*id, p.len() as u64, Sha256::digest(p).into())),
    );
    let mut bytes = vec![0; 128];
    bytes[..8].copy_from_slice(b"EMILYOBK");
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&128u32.to_le_bytes());
    bytes[16..32].copy_from_slice(PROJECT.as_bytes());
    bytes[32..36].copy_from_slice(&(values.len() as u32).to_le_bytes());
    bytes[40..48].copy_from_slice(&total.to_le_bytes());
    bytes[56..88].copy_from_slice(&digest);
    for (object, payload) in values {
        let nested = encode(PROJECT, *object, payload).unwrap();
        bytes.extend_from_slice(object.as_bytes());
        bytes.extend_from_slice(&(nested.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&nested);
    }
    let body = bytes.len() - 128;
    bytes[48..56].copy_from_slice(&(body as u64).to_le_bytes());
    seal(&mut bytes);
    bytes
}
fn same_result(bytes: &[u8], project: ProjectId) {
    let whole = verify_archive(bytes, project);
    let streamed = verify_archive_reader(&mut Cursor::new(bytes), bytes.len(), project);
    match (whole, streamed) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.objects().len(), b.objects);
            assert_eq!(a.payload_bytes(), b.payload_bytes);
            assert_eq!(a.digest(), &b.digest);
        }
        (Err(Error::ArchiveVersion(a)), Err(Error::ArchiveVersion(b))) => assert_eq!(a, b),
        (Err(Error::Version(a)), Err(Error::Version(b))) => assert_eq!(a, b),
        (Err(a), Err(b)) => assert_eq!(std::mem::discriminant(&a), std::mem::discriminant(&b)),
        _ => panic!("byte and seekable archive admission differ"),
    }
}

struct Reader<'a> {
    bytes: Cursor<&'a [u8]>,
    chunk: usize,
    reads: usize,
    seeks: usize,
    consumed: usize,
    maximum_request: usize,
    interrupt: bool,
    read_failure: Option<(usize, usize)>,
    seek_failure: Option<usize>,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], chunk: usize) -> Self {
        Self {
            bytes: Cursor::new(bytes),
            chunk,
            reads: 0,
            seeks: 0,
            consumed: 0,
            maximum_request: 0,
            interrupt: false,
            read_failure: None,
            seek_failure: None,
        }
    }
}
impl Read for Reader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        self.maximum_request = self.maximum_request.max(out.len());
        if self.interrupt && self.reads % 2 == 1 {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let mut count = out.len().min(self.chunk);
        if let Some((phase, position)) = self.read_failure
            && self.seeks == phase
        {
            let current = self.bytes.position() as usize;
            if current >= position {
                return Err(io::Error::other("synthetic read failure"));
            }
            count = count.min(position - current);
        }
        let count = self.bytes.read(&mut out[..count])?;
        self.consumed += count;
        Ok(count)
    }
}
impl Seek for Reader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.seeks += 1;
        if self.seek_failure == Some(self.seeks) {
            return Err(io::Error::other("synthetic seek failure"));
        }
        self.bytes.seek(position)
    }
}

#[test]
fn independent_archive_vector_matches_original_bytes_and_nested_metadata() {
    // Same independently produced Python struct/hashlib/zlib vector as the byte codec.
    let hex = "454d494c594f424b010000008000000001010101010101010101010101010101010000000000000013000000000000008b0000000000000074bf5d494d0be62eef120a24ce5413aaa7c1cae7b821084ba746d0191f11a0dc4031e0807d7c3fc9ab2aa3fc2851b922ccf4bfdc15e5dbf49b027df30f6be720000000007a1c3073020202020202020202020202020202027300000000000000454d494c594f424a010000006000000001010101010101010101010101010101020202020202020202020202020202021300000000000000ea0d8b4b4bc2442d1dd5b10fe97ebbcb7a23f4bb3da838fbcf588c625caef63300000000691e2ec573796e7468657469632d6172636869766500ff";
    let (pairs, extra) = hex.as_bytes().as_chunks::<2>();
    assert!(extra.is_empty());
    let bytes: Vec<_> = pairs
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    let expected = image(&[(OBJECT, b"synthetic-archive\0\xff".to_vec())]);
    assert_eq!(bytes, expected);
    same_result(&bytes, PROJECT);
    let report = verify_archive_reader(&mut Cursor::new(&bytes), bytes.len(), PROJECT).unwrap();
    assert_eq!((report.objects, report.payload_bytes), (1, 19));
}

#[test]
fn all_prefixes_byte_damage_resealed_headers_and_outer_priority_match_byte_decoder() {
    let bytes = image(&[
        (OBJECT, b"synthetic-binary\0\xff".to_vec()),
        (ObjectId::from_bytes([4; 16]), b"second".to_vec()),
    ]);
    for end in 0..bytes.len() {
        same_result(&bytes[..end], PROJECT);
    }
    for position in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[position] ^= 1;
        same_result(&changed, PROJECT);
    }
    for (position, value) in [
        (0, b'X'),
        (8, 2),
        (10, 1),
        (12, 127),
        (16, 3),
        (32, 129),
        (36, 1),
        (40, 0),
        (48, 0),
        (56, 1),
        (88, 1),
        (120, 1),
    ] {
        let mut changed = bytes.clone();
        changed[position] = value;
        header_crc(&mut changed);
        same_result(&changed, PROJECT);
    }
    let mut changed = bytes.clone();
    changed[152] ^= 1;
    assert!(matches!(
        verify_archive_reader(&mut Cursor::new(&changed), changed.len(), PROJECT),
        Err(Error::ArchiveChecksum)
    ));
    seal(&mut changed);
    assert!(matches!(
        verify_archive_reader(&mut Cursor::new(&changed), changed.len(), PROJECT),
        Err(Error::HeaderChecksum)
    ));
    same_result(&changed, PROJECT);
    same_result(&bytes, ProjectId::from_bytes([9; 16]));
}

#[test]
fn complete_nested_checks_inventory_digest_order_and_frame_lengths_cannot_be_bypassed() {
    let original = image(&[
        (OBJECT, b"first".to_vec()),
        (ObjectId::from_bytes([4; 16]), b"second".to_vec()),
    ]);
    for change in 0..10 {
        let mut bytes = original.clone();
        match change {
            0 => bytes[128..144].fill(4),
            1 => bytes[144..152].copy_from_slice(&95u64.to_le_bytes()),
            2 => bytes[144..152].copy_from_slice(&u64::MAX.to_le_bytes()),
            3 => bytes[144..152].copy_from_slice(&1024u64.to_le_bytes()),
            4 => bytes[168] ^= 1,
            5 => bytes[184] ^= 1,
            6 => bytes[248] ^= 1,
            7 => bytes[32..36].copy_from_slice(&1u32.to_le_bytes()),
            8 => bytes[40..48].copy_from_slice(&12u64.to_le_bytes()),
            9 => bytes[56] ^= 1,
            _ => unreachable!(),
        }
        if matches!(change, 4 | 5) {
            let crc = crc32fast::hash(&bytes[152..244]);
            bytes[244..248].copy_from_slice(&crc.to_le_bytes());
        }
        seal(&mut bytes);
        same_result(&bytes, PROJECT);
        assert!(verify_archive_reader(&mut Cursor::new(&bytes), bytes.len(), PROJECT).is_err());
    }
    let mut duplicate = image(&[(OBJECT, vec![]), (OBJECT, vec![])]);
    seal(&mut duplicate);
    same_result(&duplicate, PROJECT);
    assert!(verify_archive_reader(&mut Cursor::new(&duplicate), duplicate.len(), PROJECT).is_err());
}

#[test]
fn two_pass_reads_are_bounded_chunked_interrupted_and_never_cross_nested_eof() {
    let bytes = image(&[
        (OBJECT, vec![]),
        (ObjectId::from_bytes([4; 16]), vec![0xa5; 8193]),
    ]);
    for chunk in [1, 7, 8192] {
        let mut reader = Reader::new(&bytes, chunk);
        reader.interrupt = true;
        let report = verify_archive_reader(&mut reader, bytes.len(), PROJECT).unwrap();
        assert_eq!((report.objects, report.payload_bytes), (2, 8193));
        assert_eq!(reader.seeks, 2);
        assert_eq!(reader.consumed, bytes.len() * 2 - 128);
        assert!(reader.maximum_request <= 8192);
    }
    for total in [0, 127, super::super::MAX_ARCHIVE_BYTES + 1, usize::MAX] {
        let mut reader = Reader::new(&bytes, 1);
        assert!(verify_archive_reader(&mut reader, total, PROJECT).is_err());
        assert_eq!((reader.reads, reader.seeks), (0, 0));
    }
    let mut extra = bytes.clone();
    extra.extend_from_slice(&[0; 16384]);
    let mut reader = Reader::new(&extra, 17);
    assert!(matches!(
        verify_archive_reader(&mut reader, bytes.len(), PROJECT),
        Err(Error::Archive)
    ));
    assert_eq!(reader.consumed, bytes.len() + 1);
    assert_eq!(reader.seeks, 1);
}

#[test]
fn read_seek_and_exact_eof_probe_failures_remain_typed_without_partial_reports() {
    let bytes = image(&[(OBJECT, vec![0xa5; 16385])]);
    for (phase, position) in [
        (1, 0),
        (1, 50),
        (1, 128),
        (1, 1000),
        (1, bytes.len()),
        (2, 128),
        (2, 152),
        (2, 248),
        (2, bytes.len() - 1),
    ] {
        let mut reader = Reader::new(&bytes, 7);
        reader.read_failure = Some((phase, position));
        assert!(matches!(
            verify_archive_reader(&mut reader, bytes.len(), PROJECT),
            Err(Error::Io(_))
        ));
    }
    for failure in [1, 2] {
        let mut reader = Reader::new(&bytes, 7);
        reader.seek_failure = Some(failure);
        assert!(matches!(
            verify_archive_reader(&mut reader, bytes.len(), PROJECT),
            Err(Error::Io(_))
        ));
    }
    for end in [0, 50, 128, 500, bytes.len() - 1] {
        assert!(matches!(
            verify_archive_reader(&mut Cursor::new(&bytes[..end]), bytes.len(), PROJECT),
            Err(Error::Archive)
        ));
    }
    let mut reader = Cursor::new(&bytes);
    reader.set_position(17);
    assert!(verify_archive_reader(&mut reader, bytes.len(), PROJECT).is_ok());
}

#[test]
fn empty_maximum_count_and_maximum_payload_archives_remain_exact() {
    for values in [
        vec![],
        (0..128)
            .map(|i| (ObjectId::from_bytes([i; 16]), vec![]))
            .collect(),
        (0..8)
            .map(|i| (ObjectId::from_bytes([i; 16]), vec![i; MAX_PAYLOAD_BYTES]))
            .collect(),
    ] {
        let bytes = image(&values);
        same_result(&bytes, PROJECT);
        assert_eq!(
            verify_archive_reader(&mut Cursor::new(&bytes), bytes.len(), PROJECT)
                .unwrap()
                .objects,
            values.len()
        );
    }
}

#[test]
fn native_mutations_between_passes_refuse_without_namespace_cleanup() {
    for mutation in 0..6 {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("synthetic.object-archive");
        let bytes = image(&[(OBJECT, b"synthetic-private".to_vec())]);
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let saved = temp.path().join("saved");
        let result = super::super::inspect_archive_file_with(&path, PROJECT, || match mutation {
            0 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            1 => fs::hard_link(&path, &saved).unwrap(),
            2 => {
                fs::rename(&path, &saved).unwrap();
                fs::copy(&saved, &path).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            3 => File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(128)
                .unwrap(),
            4 => fs::remove_file(&path).unwrap(),
            5 => fs::write(&path, image(&[(OBJECT, b"synthetic-changed".to_vec())])).unwrap(),
            _ => unreachable!(),
        });
        assert!(result.is_err(), "native mutation {mutation}");
        if matches!(mutation, 1 | 2) {
            assert!(saved.is_file());
        }
        if mutation == 4 {
            assert!(!path.exists());
        } else {
            assert!(path.exists());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_binary_archives_match_borrowed_decoder(payloads in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..12000), 0..6)) {
        let values: Vec<_> = payloads.into_iter().enumerate().map(|(i,p)| (ObjectId::from_bytes([i as u8;16]),p)).collect();
        let bytes = image(&values);same_result(&bytes, PROJECT);
        let mut chunked = Reader::new(&bytes, 11);chunked.interrupt = true;
        let result = verify_archive_reader(&mut chunked, bytes.len(), PROJECT).unwrap();
        prop_assert_eq!(result.objects, values.len());
        prop_assert_eq!(result.payload_bytes, values.iter().map(|(_,p)| p.len() as u64).sum::<u64>());
    }
}
