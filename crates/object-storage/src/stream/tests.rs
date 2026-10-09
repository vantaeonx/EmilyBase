use super::*;
use crate::{encode, verify};
use proptest::prelude::*;
use std::io::{self, Cursor};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);

struct Chunked<'a> {
    input: Cursor<&'a [u8]>,
    chunk: usize,
    maximum_request: usize,
    calls: usize,
    interrupt: bool,
    fail_at: Option<usize>,
}
impl<'a> Chunked<'a> {
    fn new(bytes: &'a [u8], chunk: usize) -> Self {
        Self {
            input: Cursor::new(bytes),
            chunk,
            maximum_request: 0,
            calls: 0,
            interrupt: false,
            fail_at: None,
        }
    }
}
impl Read for Chunked<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        self.maximum_request = self.maximum_request.max(out.len());
        if self.interrupt && self.calls % 2 == 1 {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let position = self.input.position() as usize;
        if self.fail_at.is_some_and(|limit| position >= limit) {
            return Err(io::Error::other("synthetic reader failure"));
        }
        let mut count = self.chunk.min(out.len());
        if let Some(limit) = self.fail_at {
            count = count.min(limit - position);
        }
        self.input.read(&mut out[..count])
    }
}
fn same_result(bytes: &[u8], project: ProjectId, object: ObjectId) {
    let whole = verify(bytes, project, object);
    let stream = verify_stream(&mut Cursor::new(bytes), bytes.len(), project, object);
    match (whole, stream) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.payload().len(), b.payload_bytes);
            assert_eq!(a.sha256(), &b.sha256);
        }
        (Err(Error::Version(a)), Err(Error::Version(b))) => assert_eq!(a, b),
        (Err(a), Err(b)) => assert_eq!(std::mem::discriminant(&a), std::mem::discriminant(&b)),
        _ => panic!("whole and stream admission differ"),
    }
}
fn repair(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[..92]);
    bytes[92..96].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn streaming_known_independent_vector_preserves_complete_original_v1_bytes() {
    // Same independent Python struct/hashlib/zlib fixture as the byte decoder.
    let hex = "454d494c594f424a0100000060000000000102030405060708090a0b0c0d0e0ff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff1400000000000000446c0be5cfad1f2ae34522edaa239483d3333996d4a5ce5db151cc7299c70de400000000d342f96c73796e7468657469632d6f626a65637400e7958c";
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    assert!(remainder.is_empty());
    let bytes: Vec<_> = pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let project = ProjectId::from_bytes(std::array::from_fn(|i| i as u8));
    let object = ObjectId::from_bytes(std::array::from_fn(|i| 240 + i as u8));
    let report = verify_stream(&mut Cursor::new(&bytes), bytes.len(), project, object).unwrap();
    assert_eq!(report.payload_bytes, 20);
    let expected = "446c0be5cfad1f2ae34522edaa239483d3333996d4a5ce5db151cc7299c70de4";
    assert_eq!(
        report
            .sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        expected
    );
    assert_eq!(
        bytes,
        encode(project, object, "synthetic-object\0界".as_bytes()).unwrap()
    );
}

#[test]
fn exact_empty_scratch_transitions_and_maximum_payload_match_owned_verification() {
    for size in [0, 1, 8191, 8192, 8193, 16384, MAX_PAYLOAD_BYTES] {
        let payload: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
        let image = encode(PROJECT, OBJECT, &payload).unwrap();
        let mut reader = Chunked::new(&image, SCRATCH_BYTES);
        let report = verify_stream(&mut reader, image.len(), PROJECT, OBJECT).unwrap();
        assert_eq!(report.payload_bytes, size);
        assert_eq!(report.sha256, Sha256::digest(&payload).as_slice());
        assert_eq!(reader.input.position() as usize, image.len());
        assert!(reader.maximum_request <= SCRATCH_BYTES);
        same_result(&image, PROJECT, OBJECT);
    }
}

#[test]
fn invalid_declared_bounds_refuse_before_reading_and_never_consume_unbounded_input() {
    let bytes = encode(PROJECT, OBJECT, b"synthetic-private").unwrap();
    for declared in [
        0,
        HEADER_BYTES - 1,
        HEADER_BYTES + MAX_PAYLOAD_BYTES + 1,
        usize::MAX,
    ] {
        let mut reader = Chunked::new(&bytes, 1);
        assert!(verify_stream(&mut reader, declared, PROJECT, OBJECT).is_err());
        assert_eq!(reader.calls, 0);
    }
    let mut extra = bytes.clone();
    extra.extend_from_slice(&[0xa5; 16384]);
    let mut reader = Chunked::new(&extra, 17);
    assert!(matches!(
        verify_stream(&mut reader, bytes.len(), PROJECT, OBJECT),
        Err(Error::Format)
    ));
    assert_eq!(reader.input.position() as usize, bytes.len() + 1);
}

#[test]
fn short_chunks_and_interruptions_do_not_change_payload_or_exact_eof_admission() {
    let image = encode(PROJECT, OBJECT, &vec![0x59; 33001]).unwrap();
    for chunk in [1, 3, 7, 95, 96, 127, 8191] {
        let mut reader = Chunked::new(&image, chunk);
        reader.interrupt = true;
        let report = verify_stream(&mut reader, image.len(), PROJECT, OBJECT).unwrap();
        assert_eq!(report.payload_bytes, 33001);
        assert_eq!(reader.input.position() as usize, image.len());
        assert!(reader.maximum_request <= SCRATCH_BYTES);
    }
}

#[test]
fn injected_header_payload_and_eof_io_failure_never_returns_verified_metadata() {
    let image = encode(PROJECT, OBJECT, b"synthetic-private").unwrap();
    for fail_at in [0, 95, 96, 97, image.len()] {
        let mut reader = Chunked::new(&image, 7);
        reader.fail_at = Some(fail_at);
        assert!(
            matches!(
                verify_stream(&mut reader, image.len(), PROJECT, OBJECT),
                Err(Error::Io(_))
            ),
            "failure at {fail_at}"
        );
    }
    for end in 0..image.len() {
        let mut reader = Cursor::new(&image[..end]);
        assert!(matches!(
            verify_stream(&mut reader, image.len(), PROJECT, OBJECT),
            Err(Error::Format)
        ));
    }
}

#[test]
fn prefixes_corruption_repaired_header_metadata_and_foreign_scopes_match_byte_decoder() {
    let original = encode(PROJECT, OBJECT, &[0x42; 257]).unwrap();
    for end in 0..original.len() {
        same_result(&original[..end], PROJECT, OBJECT);
    }
    for byte in 0..original.len() {
        let mut corrupt = original.clone();
        corrupt[byte] ^= 1;
        same_result(&corrupt, PROJECT, OBJECT);
        if byte < 92 {
            repair(&mut corrupt);
            same_result(&corrupt, PROJECT, OBJECT);
        }
    }
    for length in [0, 1, 256, 258, MAX_PAYLOAD_BYTES as u64, u64::MAX] {
        let mut changed = original.clone();
        changed[48..56].copy_from_slice(&length.to_le_bytes());
        repair(&mut changed);
        same_result(&changed, PROJECT, OBJECT);
    }
    same_result(&original, ProjectId::from_bytes([3; 16]), OBJECT);
    same_result(&original, PROJECT, ObjectId::from_bytes([4; 16]));
    let mut trailing = original;
    trailing.push(0);
    same_result(&trailing, PROJECT, OBJECT);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_binary_readers_match_whole_object_admission_and_enforce_declared_length(
        project in any::<[u8;16]>(), object in any::<[u8;16]>(),
        payload in prop::collection::vec(any::<u8>(),0..33000),
        chunk in 1usize..8193, change in any::<usize>(),
    ) {
        let project=ProjectId::from_bytes(project);let object=ObjectId::from_bytes(object);
        let image=encode(project,object,&payload).unwrap();
        let mut reader=Chunked::new(&image,chunk);
        let report=verify_stream(&mut reader,image.len(),project,object).unwrap();
        prop_assert_eq!(report.payload_bytes,payload.len());
        let expected: [u8;32]=Sha256::digest(&payload).into();
        prop_assert_eq!(report.sha256,expected);
        prop_assert!(reader.maximum_request<=SCRATCH_BYTES);
        for declared in [image.len()-1,image.len()+1] {
            prop_assert!(verify_stream(&mut Cursor::new(&image),declared,project,object).is_err());
        }
        let mut corrupt=image;
        let position=change%corrupt.len();corrupt[position]^=1;
        same_result(&corrupt,project,object);
        if position<92 {repair(&mut corrupt);same_result(&corrupt,project,object);}
    }
}
