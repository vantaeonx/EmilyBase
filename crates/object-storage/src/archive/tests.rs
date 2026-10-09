use super::*;
use crate::{ProjectDirectory, encode};
use proptest::prelude::*;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);
fn snapshot(values: &[(ObjectId, &[u8])]) -> ObjectSnapshot {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("objects");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let mut owner = ProjectDirectory::initialize(&path, PROJECT).unwrap();
    for (id, payload) in values {
        owner.put(*id, payload).unwrap();
    }
    owner.capture().unwrap()
}
fn image() -> Vec<u8> {
    encode_archive(&snapshot(&[
        (OBJECT, b"old!"),
        (ObjectId::from_bytes([4; 16]), b"more"),
    ]))
    .unwrap()
}
fn header_crc(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}
fn seal(bytes: &mut [u8]) {
    let hash = Sha256::digest(&bytes[128..]);
    bytes[88..120].copy_from_slice(&hash);
    header_crc(bytes);
}
fn inner_crc(bytes: &mut [u8]) {
    let crc = crc32fast::hash(&bytes[..92]);
    bytes[92..96].copy_from_slice(&crc.to_le_bytes());
}
fn private(path: &std::path::Path, bytes: &[u8]) {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn independent_python_struct_hashlib_zlib_archive_vector_matches_all_bytes() {
    let snapshot = snapshot(&[(OBJECT, b"synthetic-archive\0\xff")]);
    let bytes = encode_archive(&snapshot).unwrap();
    let expected = "454d494c594f424b010000008000000001010101010101010101010101010101010000000000000013000000000000008b0000000000000074bf5d494d0be62eef120a24ce5413aaa7c1cae7b821084ba746d0191f11a0dc4031e0807d7c3fc9ab2aa3fc2851b922ccf4bfdc15e5dbf49b027df30f6be720000000007a1c3073020202020202020202020202020202027300000000000000454d494c594f424a010000006000000001010101010101010101010101010101020202020202020202020202020202021300000000000000ea0d8b4b4bc2442d1dd5b10fe97ebbcb7a23f4bb3da838fbcf588c625caef63300000000691e2ec573796e7468657469632d6172636869766500ff";
    assert_eq!(
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        expected
    );
    let view = verify_archive(&bytes, PROJECT).unwrap();
    assert_eq!(view.project(), PROJECT);
    assert_eq!(view.payload_bytes(), 19);
    assert_eq!(view.objects().len(), 1);
    assert_eq!(view.objects()[0].object(), OBJECT);
    assert_eq!(view.objects()[0].payload(), b"synthetic-archive\0\xff");
    assert_eq!(view.digest(), snapshot.inventory().digest());
    assert_eq!(view.objects()[0].payload().as_ptr(), bytes[248..].as_ptr());
    assert_eq!(encode_verified_archive(&view).unwrap(), bytes);
    assert!(!format!("{view:?} {:?}", view.objects()[0]).contains("synthetic-archive"));
}

#[test]
fn every_prefix_and_single_byte_corruption_refuses_without_exposing_partial_objects() {
    let bytes = image();
    for end in 0..bytes.len() {
        assert!(
            verify_archive(&bytes[..end], PROJECT).is_err(),
            "prefix {end}"
        );
    }
    for i in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[i] ^= 1;
        assert!(verify_archive(&changed, PROJECT).is_err(), "byte {i}");
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(verify_archive(&trailing, PROJECT).is_err());
}

#[test]
fn resealed_header_still_refuses_unknown_versions_flags_lengths_counts_and_foreign_scope() {
    let original = image();
    for (offset, value) in [(0, b'X'), (10, 1), (12, 127), (36, 1), (120, 1)] {
        let mut bytes = original.clone();
        bytes[offset] = value;
        header_crc(&mut bytes);
        assert!(matches!(
            verify_archive(&bytes, PROJECT),
            Err(Error::Archive)
        ));
    }
    let mut bytes = original.clone();
    bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
    header_crc(&mut bytes);
    assert!(matches!(
        verify_archive(&bytes, PROJECT),
        Err(Error::ArchiveVersion(2))
    ));
    let mut bytes = original.clone();
    bytes[16] ^= 1;
    header_crc(&mut bytes);
    assert!(matches!(verify_archive(&bytes, PROJECT), Err(Error::Scope)));
    assert!(matches!(
        verify_archive(&original, ProjectId::from_bytes([9; 16])),
        Err(Error::Scope)
    ));
    for offset in [40, 48] {
        let mut bytes = original.clone();
        bytes[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        header_crc(&mut bytes);
        assert!(matches!(verify_archive(&bytes, PROJECT), Err(Error::Limit)));
    }
    let mut bytes = original.clone();
    bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    header_crc(&mut bytes);
    assert!(matches!(verify_archive(&bytes, PROJECT), Err(Error::Limit)));
    for count in [0u32, 1, 3] {
        let mut bytes = original.clone();
        bytes[32..36].copy_from_slice(&count.to_le_bytes());
        header_crc(&mut bytes);
        assert!(verify_archive(&bytes, PROJECT).is_err());
    }
    let mut bytes = original.clone();
    bytes[40..48].copy_from_slice(&0u64.to_le_bytes());
    header_crc(&mut bytes);
    assert!(verify_archive(&bytes, PROJECT).is_err());
    for offset in [56, 88] {
        let mut bytes = original.clone();
        bytes[offset] ^= 1;
        header_crc(&mut bytes);
        assert!(matches!(
            verify_archive(&bytes, PROJECT),
            Err(Error::ArchiveChecksum)
        ));
    }
}

#[test]
fn resealed_body_still_requires_exact_frames_sorted_unique_ids_and_nested_expected_scope() {
    let original = image();
    let frame_bytes = 24 + 96 + 4;
    let mut swapped = original.clone();
    let first = original[128..128 + frame_bytes].to_vec();
    let second = original[128 + frame_bytes..].to_vec();
    swapped[128..128 + frame_bytes].copy_from_slice(&second);
    swapped[128 + frame_bytes..].copy_from_slice(&first);
    seal(&mut swapped);
    assert!(matches!(
        verify_archive(&swapped, PROJECT),
        Err(Error::Archive)
    ));
    let mut duplicate = original.clone();
    let start = 128 + frame_bytes;
    duplicate[start..start + 16].copy_from_slice(OBJECT.as_bytes());
    duplicate[start + 24 + 32..start + 24 + 48].copy_from_slice(OBJECT.as_bytes());
    inner_crc(&mut duplicate[start + 24..]);
    seal(&mut duplicate);
    assert!(matches!(
        verify_archive(&duplicate, PROJECT),
        Err(Error::Archive)
    ));
    for length in [0u64, 95, 96, u64::MAX] {
        let mut bytes = original.clone();
        bytes[144..152].copy_from_slice(&length.to_le_bytes());
        seal(&mut bytes);
        assert!(verify_archive(&bytes, PROJECT).is_err());
    }
    let mut foreign = original.clone();
    foreign[152 + 16..152 + 32].fill(9);
    inner_crc(&mut foreign[152..]);
    seal(&mut foreign);
    assert!(matches!(
        verify_archive(&foreign, PROJECT),
        Err(Error::Scope)
    ));
    let mut wrong_id = original.clone();
    wrong_id[128] ^= 1;
    seal(&mut wrong_id);
    assert!(verify_archive(&wrong_id, PROJECT).is_err());
    let mut changed = original;
    changed[152..152 + 100].copy_from_slice(&encode(PROJECT, OBJECT, b"new!").unwrap());
    seal(&mut changed);
    assert!(matches!(
        verify_archive(&changed, PROJECT),
        Err(Error::ArchiveChecksum)
    ));
}

#[test]
fn canonical_empty_full_count_and_maximum_payload_archives_round_trip() {
    let empty = encode_archive(&snapshot(&[])).unwrap();
    assert_eq!(empty.len(), ARCHIVE_HEADER_BYTES);
    assert!(
        verify_archive(&empty, PROJECT)
            .unwrap()
            .objects()
            .is_empty()
    );
    let ids = (0..128)
        .map(|key| (ObjectId::from_bytes([key; 16]), &[][..]))
        .collect::<Vec<_>>();
    let bytes = encode_archive(&snapshot(&ids)).unwrap();
    assert_eq!(bytes.len(), 128 + 128 * 120);
    assert_eq!(
        verify_archive(&bytes, PROJECT).unwrap().objects().len(),
        128
    );
    let payload = vec![0x59; MAX_PAYLOAD_BYTES];
    let mut parts = (0..8)
        .map(|key| (ObjectId::from_bytes([key; 16]), payload.as_slice()))
        .collect::<Vec<_>>();
    parts.extend((8..128).map(|key| (ObjectId::from_bytes([key; 16]), &[][..])));
    let snapshot = snapshot(&parts);
    let bytes = encode_archive(&snapshot).unwrap();
    let view = verify_archive(&bytes, PROJECT).unwrap();
    assert_eq!(bytes.len(), MAX_ARCHIVE_BYTES);
    assert_eq!(view.payload_bytes(), MAX_INVENTORY_BYTES);
    assert!(
        view.objects()
            .iter()
            .take(8)
            .all(|object| object.payload() == payload)
    );
    assert!(
        view.objects()
            .iter()
            .skip(8)
            .all(|object| object.payload().is_empty())
    );
    assert_eq!(encode_verified_archive(&view).unwrap(), bytes);
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("maximum.object-archive");
    private(&path, &bytes);
    let report = inspect_archive_file(&path, PROJECT).unwrap();
    assert_eq!(report.objects, 128);
    assert_eq!(report.payload_bytes, MAX_INVENTORY_BYTES);
    assert_eq!(report.digest, *snapshot.inventory().digest());
    assert_eq!(fs::metadata(path).unwrap().len(), MAX_ARCHIVE_BYTES as u64);
    assert!(matches!(
        verify_archive(&vec![0; MAX_ARCHIVE_BYTES + 1], PROJECT),
        Err(Error::Limit)
    ));
}

#[test]
fn native_archive_inspection_is_readonly_private_bounded_and_metadata_only() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("synthetic.object-archive");
    let bytes = image();
    private(&path, &bytes);
    let report = inspect_archive_file(&path, PROJECT).unwrap();
    assert_eq!(report.objects, 2);
    assert_eq!(report.payload_bytes, 8);
    assert_eq!(
        &report.digest,
        verify_archive(&bytes, PROJECT).unwrap().digest()
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(inspect_archive_file(&path, ProjectId::from_bytes([9; 16])).is_err());
    let alias = temp.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(inspect_archive_file(&alias, PROJECT).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    assert!(inspect_archive_file(&path, PROJECT).is_err());
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(inspect_archive_file(&path, PROJECT).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(inspect_archive_file(&path, PROJECT).unwrap(), report);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let file = File::options().write(true).open(&path).unwrap();
    file.set_len((MAX_ARCHIVE_BYTES + 1) as u64).unwrap();
    assert!(matches!(
        inspect_archive_file(&path, PROJECT),
        Err(Error::Limit)
    ));
    file.set_len(0).unwrap();
    assert!(inspect_archive_file(&path, PROJECT).is_err());
    let fifo = temp.path().join("fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg("-m")
            .arg("600")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(inspect_archive_file(&fifo, PROJECT).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn generated_canonical_archives_preserve_complete_sorted_binary_content(
        values in prop::collection::btree_map(0u8..16,prop::collection::vec(any::<u8>(),0..257),0..9)
    ) {
        let parts=values.iter().map(|(&key,value)|(ObjectId::from_bytes([key;16]),value.as_slice())).collect::<Vec<_>>();
        let captured=snapshot(&parts);let bytes=encode_archive(&captured).unwrap();let archive=verify_archive(&bytes,PROJECT).unwrap();
        prop_assert_eq!(archive.objects().len(),values.len());prop_assert_eq!(archive.payload_bytes(),values.values().map(|v|v.len() as u64).sum::<u64>());
        prop_assert_eq!(archive.digest(),captured.inventory().digest());
        for (object,(&key,value)) in archive.objects().iter().zip(&values) {prop_assert_eq!(object.object(),ObjectId::from_bytes([key;16]));prop_assert_eq!(object.payload(),value);}
        prop_assert_eq!(encode_verified_archive(&archive).unwrap(),bytes);
    }
    #[test]
    fn untrusted_arbitrary_archives_never_expose_noncanonical_views(bytes in prop::collection::vec(any::<u8>(),0..4097)) {
        if let Ok(archive)=verify_archive(&bytes,PROJECT) {prop_assert_eq!(encode_verified_archive(&archive).unwrap(),bytes);}
    }
}
