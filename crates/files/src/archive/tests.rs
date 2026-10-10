use super::*;
use crate::{FileId, FileSnapshot, FileStore, TEST_IO};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const FILE: FileId = FileId::from_bytes([2; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([3; 16]);
fn setup(path: &Path, compacted: bool) -> FileStore {
    let mut db = Database::create(path.join("metadata")).unwrap();
    if compacted {
        db.compact().unwrap();
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path.join("objects"))
        .unwrap();
    let objects = ProjectDirectory::initialize(path.join("objects"), PROJECT).unwrap();
    FileStore::initialize(db, objects, FileQuota::new(8, 32768).unwrap()).unwrap()
}
fn bytes(snapshot: &FileSnapshot) -> Vec<u8> {
    let mut reader = FileArchiveReader::from_snapshot(snapshot).unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len(), reader.encoded_bytes());
    bytes
}
fn assemble(metadata: &[u8], objects: &[u8], project: ProjectId) -> Vec<u8> {
    let report = emilybase_backup::inspect_bytes(metadata).unwrap();
    let header = header::encode(
        project,
        &header::Header {
            database_id: report.database_id,
            last_transaction: report.last_transaction,
            metadata_bytes: metadata.len(),
            object_bytes: objects.len(),
            metadata_hash: Sha256::digest(metadata).into(),
            object_hash: Sha256::digest(objects).into(),
        },
    );
    let mut result = header.to_vec();
    result.extend_from_slice(metadata);
    result.extend_from_slice(objects);
    result
}
fn crc(bytes: &mut [u8]) {
    let hash = crc32fast::hash(&bytes[..188]);
    bytes[188..192].copy_from_slice(&hash.to_le_bytes());
}
fn outer_hashes(bytes: &mut [u8]) {
    let middle = 192 + u64::from_le_bytes(bytes[56..64].try_into().unwrap()) as usize;
    let metadata: [u8; 32] = Sha256::digest(&bytes[192..middle]).into();
    let objects: [u8; 32] = Sha256::digest(&bytes[middle..]).into();
    bytes[72..104].copy_from_slice(&metadata);
    bytes[104..136].copy_from_slice(&objects);
    crc(bytes);
}

#[test]
fn canonical_pair_wire_partition_checksums_metadata_orphans_and_borrowed_reencoding() {
    let _serial = TEST_IO.lock().unwrap();
    for compacted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut store = setup(temp.path(), compacted);
        let info = store
            .publish(
                FILE,
                OBJECT,
                [4; 16],
                "synthetic-display",
                &vec![0x53; 8193],
            )
            .unwrap();
        store
            .objects
            .put(ObjectId::from_bytes([5; 16]), b"orphan")
            .unwrap();
        let snapshot = store.capture().unwrap();
        let encoded = bytes(&snapshot);
        assert_eq!(&encoded[..8], b"EMILYFBK");
        assert_eq!(&encoded[8..12], &[1, 0, 192, 0]);
        assert_eq!(&encoded[16..32], PROJECT.as_bytes());
        assert_eq!(&encoded[32..48], snapshot.metadata_report().database_id);
        assert_eq!(
            u64::from_le_bytes(encoded[48..56].try_into().unwrap()),
            snapshot.metadata_report().last_transaction
        );
        assert_eq!(
            u64::from_le_bytes(encoded[56..64].try_into().unwrap()),
            snapshot.metadata_bytes().len() as u64
        );
        let expected_objects =
            emilybase_object_storage::encode_archive(snapshot.objects()).unwrap();
        assert_eq!(
            encoded,
            assemble(snapshot.metadata_bytes(), &expected_objects, PROJECT)
        );
        store.rename(FILE, info.revision(), "later").unwrap();
        drop(store);
        drop(snapshot);
        let verified = verify_file_archive(&encoded, PROJECT).unwrap();
        assert_eq!(verified.files(), &[info]);
        assert_eq!(verified.project(), PROJECT);
        assert_eq!(
            verified.metadata_report().wal_version,
            if compacted { 2 } else { 1 }
        );
        assert_eq!(verified.objects().objects().len(), 2);
        assert_eq!(verified.objects().payload_bytes(), 8199);
        let mut reader = FileArchiveReader::from_verified(&verified).unwrap();
        let mut copy = Vec::new();
        reader.read_to_end(&mut copy).unwrap();
        assert_eq!(copy, encoded);
        for debug in [format!("{verified:?}"), format!("{reader:?}")] {
            assert!(!debug.contains("synthetic-display"));
            assert!(!debug.contains(&PROJECT.to_string()));
        }
        assert!(matches!(
            verify_file_archive(&encoded, ProjectId::from_bytes([9; 16])),
            Err(Error::Scope)
        ));
    }
}

#[test]
fn fixed_header_mutations_truncation_overflow_gaps_and_trailing_data_refuse() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), false);
    let snapshot = store.capture().unwrap();
    let encoded = bytes(&snapshot);
    for offset in [
        0, 7, 8, 9, 10, 11, 12, 15, 16, 31, 32, 47, 48, 55, 56, 63, 64, 71, 72, 103, 104, 135, 136,
        187, 188, 191,
    ] {
        let mut bad = encoded.clone();
        bad[offset] ^= 0x80;
        if offset < 188 {
            crc(&mut bad);
        }
        assert!(
            verify_file_archive(&bad, PROJECT).is_err(),
            "offset={offset}"
        );
    }
    let mut bad_crc = encoded.clone();
    bad_crc[188] ^= 1;
    assert!(matches!(
        verify_file_archive(&bad_crc, PROJECT),
        Err(Error::ArchiveChecksum)
    ));
    for range in [48..56, 56..64, 64..72] {
        for value in [0u64, 1, u64::MAX] {
            let mut bad = encoded.clone();
            bad[range.clone()].copy_from_slice(&value.to_le_bytes());
            crc(&mut bad);
            assert!(verify_file_archive(&bad, PROJECT).is_err());
        }
    }
    for length in [0, 1, 8, 191, 192, 193, encoded.len() - 1] {
        assert!(verify_file_archive(&encoded[..length], PROJECT).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(verify_file_archive(&trailing, PROJECT).is_err());
    let mut shifted = encoded.clone();
    let meta = u64::from_le_bytes(shifted[56..64].try_into().unwrap());
    let obj = u64::from_le_bytes(shifted[64..72].try_into().unwrap());
    shifted[56..64].copy_from_slice(&(meta + 1).to_le_bytes());
    shifted[64..72].copy_from_slice(&(obj - 1).to_le_bytes());
    outer_hashes(&mut shifted);
    assert!(verify_file_archive(&shifted, PROJECT).is_err());
}

#[test]
fn outer_checksum_precedes_nested_parsers_and_rechecksummed_nested_corruption_refuses() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), false);
    let snapshot = store.capture().unwrap();
    let encoded = bytes(&snapshot);
    let middle = 192 + snapshot.metadata_bytes().len();
    for offset in [192, middle] {
        let mut bad = encoded.clone();
        bad[offset] ^= 0x80;
        assert!(matches!(
            verify_file_archive(&bad, PROJECT),
            Err(Error::ArchiveChecksum)
        ));
        outer_hashes(&mut bad);
        assert!(if offset == 192 {
            matches!(verify_file_archive(&bad, PROJECT), Err(Error::Backup(_)))
        } else {
            matches!(verify_file_archive(&bad, PROJECT), Err(Error::Objects(_)))
        });
    }
}

#[test]
fn valid_independent_components_with_missing_changed_or_over_quota_objects_do_not_form_valid_pair()
{
    let _serial = TEST_IO.lock().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut source = setup(a.path(), false);
    source
        .publish(FILE, OBJECT, [4; 16], "synthetic", b"synthetic")
        .unwrap();
    let state = source.quota_state().unwrap();
    source
        .set_quota(state, FileQuota::new(1, 9).unwrap())
        .unwrap();
    let expected = source.capture().unwrap();
    let mut other = setup(b.path(), false);
    let empty = other.capture().unwrap();
    let objects = emilybase_object_storage::encode_archive(empty.objects()).unwrap();
    assert!(matches!(
        verify_file_archive(
            &assemble(expected.metadata_bytes(), &objects, PROJECT),
            PROJECT
        ),
        Err(Error::Corrupt)
    ));
    other.objects.put(OBJECT, b"different").unwrap();
    let changed = other.capture().unwrap();
    let objects = emilybase_object_storage::encode_archive(changed.objects()).unwrap();
    assert!(matches!(
        verify_file_archive(
            &assemble(expected.metadata_bytes(), &objects, PROJECT),
            PROJECT
        ),
        Err(Error::Corrupt)
    ));
    other
        .objects
        .put(ObjectId::from_bytes([6; 16]), b"")
        .unwrap();
    let extra = other.capture().unwrap();
    let objects = emilybase_object_storage::encode_archive(extra.objects()).unwrap();
    assert!(matches!(
        verify_file_archive(
            &assemble(expected.metadata_bytes(), &objects, PROJECT),
            PROJECT
        ),
        Err(Error::Quota)
    ));
    // Independently valid original-engine metadata with a different private scope
    // cannot be admitted merely because its generic backup checksum is correct.
    let raw = tempfile::tempdir().unwrap();
    let mut db = Database::create(raw.path().join("raw")).unwrap();
    let metadata = emilybase_backup::encode(&db.committed_wal().unwrap()).unwrap();
    assert!(matches!(
        verify_file_archive(&assemble(&metadata, &objects, PROJECT), PROJECT),
        Err(Error::Corrupt)
    ));
}

#[test]
fn negative_and_overflow_seek_preserve_position_and_past_eof_is_empty() {
    let _serial = TEST_IO.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut store = setup(temp.path(), false);
    let snapshot = store.capture().unwrap();
    let mut reader = FileArchiveReader::from_snapshot(&snapshot).unwrap();
    assert!(reader.seek(SeekFrom::Current(-1)).is_err());
    assert_eq!(reader.stream_position().unwrap(), 0);
    assert_eq!(reader.seek(SeekFrom::Start(u64::MAX)).unwrap(), u64::MAX);
    assert!(reader.seek(SeekFrom::Current(1)).is_err());
    assert_eq!(reader.stream_position().unwrap(), u64::MAX);
    let mut out = [0x53; 32];
    assert_eq!(reader.read(&mut out).unwrap(), 0);
    assert_eq!(out, [0x53; 32]);
    assert!(reader.seek(SeekFrom::End(i64::MIN)).is_err());
    assert_eq!(reader.stream_position().unwrap(), u64::MAX);
    reader.rewind().unwrap();
    assert_eq!(reader.read(&mut out).unwrap(), 32);
    assert_eq!(&out[..8], b"EMILYFBK");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_reader_seek_slices_match_independent_flat_image(
        compacted in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..4097),
        operations in prop::collection::vec((0u8..3,any::<u16>(),0usize..8193),1..49),
    ) {
        let _serial=TEST_IO.lock().unwrap();let temp=tempfile::tempdir().unwrap();let mut store=setup(temp.path(),compacted);
        store.publish(FILE,OBJECT,[4;16],"synthetic",&payload).unwrap();let snapshot=store.capture().unwrap();let encoded=bytes(&snapshot);
        let mut reader=FileArchiveReader::from_snapshot(&snapshot).unwrap();let mut position=0;
        for (mode,number,count) in operations {
            if mode==0 {position=number as usize;prop_assert_eq!(reader.seek(SeekFrom::Start(position as u64)).unwrap(),position as u64);}
            else if mode==1 {position=encoded.len()+(number%32) as usize;reader.seek(SeekFrom::End((number%32) as i64)).unwrap();}
            else {
                let mut out=vec![0x53;count];let n=reader.read(&mut out).unwrap();let expected=encoded.len().saturating_sub(position).min(count);
                prop_assert_eq!(n,expected);prop_assert_eq!(&out[..n],&encoded[position.min(encoded.len())..position.min(encoded.len())+n]);
                prop_assert!(out[n..].iter().all(|b|*b==0x53));position+=n;
            }
            prop_assert_eq!(reader.stream_position().unwrap(),position as u64);
        }
        let verified=verify_file_archive(&encoded,PROJECT).unwrap();prop_assert_eq!(verified.objects().objects()[0].payload(),payload);
    }
    #[test]
    fn arbitrary_bounded_bytes_never_panic_or_create_authority(input in prop::collection::vec(any::<u8>(),0..32769)) {
        let _=verify_file_archive(&input,PROJECT);
    }
}

#[test]
#[ignore = "opt-in synthetic paired-archive fuzz corpus generator"]
fn file_archive_corpus() {
    let _serial = TEST_IO.lock().unwrap();
    let path = std::env::var_os("EMILYBASE_FILE_ARCHIVE_CORPUS")
        .expect("explicit synthetic corpus directory");
    fs::create_dir_all(&path).unwrap();
    let path = std::path::PathBuf::from(path);
    for compacted in [false, true] {
        for shape in 0..4 {
            let temp = tempfile::tempdir().unwrap();
            let mut store = setup(temp.path(), compacted);
            if shape > 0 {
                let info = store
                    .publish(FILE, OBJECT, [4; 16], "synthetic", &vec![0x53; shape * 257])
                    .unwrap();
                if shape == 2 {
                    store.remove(FILE, info.revision()).unwrap();
                }
                if shape == 3 {
                    store.rename(FILE, info.revision(), "changed").unwrap();
                    let state = store.quota_state().unwrap();
                    store
                        .set_quota(state, FileQuota::new(1, 1024).unwrap())
                        .unwrap();
                }
            }
            let snapshot = store.capture().unwrap();
            let encoded = bytes(&snapshot);
            for mode in 0..2u8 {
                let mut seed = vec![mode];
                seed.extend_from_slice(&encoded);
                fs::write(
                    path.join(format!(
                        "wal{}-shape{shape}-mode{mode}",
                        if compacted { 2 } else { 1 }
                    )),
                    seed,
                )
                .unwrap();
            }
        }
    }
}
