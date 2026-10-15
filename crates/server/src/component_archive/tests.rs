use super::*;
use proptest::prelude::*;

fn seal(bytes: &mut [u8]) {
    let length = bytes.len() - HEADER;
    bytes[32..40].copy_from_slice(&(length as u64).to_le_bytes());
    let hash = Sha256::digest(&bytes[HEADER..]);
    bytes[40..72].copy_from_slice(&hash);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}
fn empty() -> Vec<u8> {
    let mut registry = vec![0; 128];
    registry[..8].copy_from_slice(b"EMILYREG");
    registry[8..10].copy_from_slice(&1u16.to_le_bytes());
    registry[10..12].copy_from_slice(&128u16.to_le_bytes());
    registry[24..56].copy_from_slice(&Sha256::digest([]));
    let crc = crc32fast::hash(&registry[..124]);
    registry[124..].copy_from_slice(&crc.to_le_bytes());
    let accounts = account_bundle::encode(&registry, Vec::new()).unwrap();
    let manifest = AccountFileRootManifest::new(Vec::new(), Vec::new(), 50)
        .unwrap()
        .to_bytes()
        .unwrap();
    let mut bytes = vec![0; HEADER];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&128u16.to_le_bytes());
    bytes[16..20].copy_from_slice(&(manifest.len() as u32).to_le_bytes());
    bytes[24..32].copy_from_slice(&(accounts.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&accounts);
    seal(&mut bytes);
    bytes
}

#[test]
fn independent_empty_envelope_has_exact_partition_and_borrowed_roundtrip() {
    use std::io::Read;
    let bytes = empty();
    // Independently constructed with Python struct/hashlib/zlib over the frozen
    // empty nested registry/account formats and canonical manifest, reset_at50.
    assert_eq!(bytes.len(), 484);
    assert_eq!(u32_at(&bytes, 124), 2800842180);
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "f983e4f3a626b67cc1f49dba2da181f02af9ca8c1e75a096076c06c157a388f4"
    );
    let verified = verify_account_file_archive(&bytes).unwrap();
    assert_eq!(MAX_ACCOUNT_FILE_ARCHIVE_BYTES, 128 * 1024 * 1024 + 4672);
    assert_eq!(verified.manifest().reset_at(), 50);
    assert_eq!(verified.physical_objects(), 0);
    assert!(verified.files().is_empty());
    assert!(verified.file_bytes(0).is_none());
    assert_eq!(verified.copied_bytes() + HEADER, bytes.len());
    let mut encoded = Vec::new();
    AccountFileArchiveReader::from_verified(&verified)
        .unwrap()
        .read_to_end(&mut encoded)
        .unwrap();
    assert_eq!(encoded, bytes);
}

#[test]
fn outer_header_mutations_and_raw_truncations_refuse_without_panic() {
    let baseline = empty();
    for at in 0..HEADER {
        let mut bytes = baseline.clone();
        bytes[at] ^= 1;
        assert!(verify_account_file_archive(&bytes).is_err(), "offset={at}");
    }
    for end in 0..baseline.len() {
        assert!(
            verify_account_file_archive(&baseline[..end]).is_err(),
            "cut={end}"
        );
    }
    let mut bytes = baseline.clone();
    bytes.push(0);
    assert!(verify_account_file_archive(&bytes).is_err());
    seal(&mut bytes);
    assert!(verify_account_file_archive(&bytes).is_err());
}

#[test]
fn oversized_complete_input_is_refused_before_invalid_magic_or_nested_work() {
    let bytes = vec![0; MAX_ACCOUNT_FILE_ARCHIVE_BYTES + 1];
    assert!(matches!(
        verify_account_file_archive(&bytes),
        Err(Error::Limit)
    ));
}

#[test]
fn repaired_invalid_counts_lengths_reserved_fields_and_versions_refuse() {
    let baseline = empty();
    for (at, width, value) in [
        (12, 4, 5u64),
        (16, 4, 32769),
        (24, 8, u64::MAX),
        (24, 8, 0),
        (32, 8, u64::MAX),
        (16, 4, 0),
        (12, 4, 1),
        (16, 4, 32768),
    ] {
        let mut bytes = baseline.clone();
        bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
        let crc = crc32fast::hash(&bytes[..124]);
        bytes[124..128].copy_from_slice(&crc.to_le_bytes());
        assert!(verify_account_file_archive(&bytes).is_err());
    }
    for at in (20..24).chain(72..124) {
        let mut bytes = baseline.clone();
        bytes[at] = 1;
        seal(&mut bytes);
        assert!(verify_account_file_archive(&bytes).is_err());
    }
    let mut bytes = baseline.clone();
    bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
    seal(&mut bytes);
    assert!(matches!(
        verify_account_file_archive(&bytes),
        Err(Error::ComponentArchiveVersion(2))
    ));
}

#[test]
fn outer_body_checksum_precedes_nested_decoding_and_repaired_nested_corruption_refuses() {
    let baseline = empty();
    for at in HEADER..baseline.len() {
        let mut bytes = baseline.clone();
        bytes[at] ^= 1;
        assert!(matches!(
            verify_account_file_archive(&bytes),
            Err(Error::ComponentArchiveChecksum)
        ));
        seal(&mut bytes);
        assert!(verify_account_file_archive(&bytes).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_bounded_inputs_and_repaired_headers_never_panic(mut bytes in prop::collection::vec(any::<u8>(),0..8193)) {
        let _=verify_account_file_archive(&bytes);
        if bytes.len()>=HEADER {seal(&mut bytes);let _=verify_account_file_archive(&bytes);}
    }
}
