use super::*;
use crate::{
    AccountFileArchiveReader, AccountFileRootManifest, FileRootBinding, verify_account_file_archive,
};
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom};

fn encode(snapshot: &crate::AccountFileSnapshot) -> Vec<u8> {
    let mut bytes = Vec::new();
    AccountFileArchiveReader::from_snapshot(snapshot)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
fn seal(bytes: &mut [u8]) {
    let n = bytes.len() - 128;
    bytes[32..40].copy_from_slice(&(n as u64).to_le_bytes());
    let hash = Sha256::digest(&bytes[128..]);
    bytes[40..72].copy_from_slice(&hash);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}
fn replace_manifest(bytes: &[u8], manifest: &AccountFileRootManifest) -> Vec<u8> {
    let old = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut out = bytes[..128].to_vec();
    let image = manifest.to_bytes().unwrap();
    out[16..20].copy_from_slice(&(image.len() as u32).to_le_bytes());
    out.extend_from_slice(&image);
    out.extend_from_slice(&bytes[128 + old..]);
    seal(&mut out);
    out
}

#[test]
fn joined_bytes_roundtrip_match_native_components_and_independent_framing_without_source_changes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (compact, count) in [(false, 0), (false, 1), (true, 4)] {
        let temp = tempfile::tempdir().unwrap();
        let (f, _, paths) = prepare(temp.path(), count, compact);
        let before = history(&f, &paths);
        let mut root = AccountRoot::open(&f.root, pool()).unwrap();
        let mut files = open(&paths);
        let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
        let bytes = encode(&snapshot);
        drop(files);
        drop(root);
        let verified = verify_account_file_archive(&bytes).unwrap();
        assert_eq!(verified.manifest(), snapshot.manifest());
        assert_eq!(verified.account_bytes(), snapshot.accounts().bundle_bytes());
        assert_eq!(verified.copied_bytes(), snapshot.copied_bytes());
        assert_eq!(verified.physical_objects(), snapshot.physical_objects());
        assert_eq!(verified.encoded_bytes(), bytes.len());
        assert_eq!(verified.files().len(), count);
        assert!(!format!("{verified:?}").contains("synthetic"));
        let mut at =
            128 + snapshot.manifest_bytes().len() + snapshot.accounts().bundle_bytes().len();
        for (index, file) in snapshot.files().iter().enumerate() {
            let mut expected = Vec::new();
            FileArchiveReader::from_snapshot(file)
                .unwrap()
                .read_to_end(&mut expected)
                .unwrap();
            assert_eq!(&bytes[at..at + 32], file.project().to_string().as_bytes());
            assert_eq!(
                u64::from_le_bytes(bytes[at + 32..at + 40].try_into().unwrap()) as usize,
                expected.len()
            );
            assert!(bytes[at + 40..at + 48].iter().all(|&n| n == 0));
            at += 48;
            assert_eq!(&bytes[at..at + expected.len()], expected);
            assert_eq!(verified.file_bytes(index).unwrap(), expected);
            at += expected.len();
        }
        assert_eq!(at, bytes.len());
        assert!(verified.file_bytes(count).is_none());
        let mut copied = Vec::new();
        AccountFileArchiveReader::from_verified(&verified)
            .unwrap()
            .read_to_end(&mut copied)
            .unwrap();
        assert_eq!(copied, bytes);
        assert_eq!(history(&f, &paths), before);
    }
}

#[test]
fn borrowed_encoder_reads_and_seeks_across_every_frame_boundary_and_preserves_failed_seek_position()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, _, paths) = prepare(temp.path(), 2, true);
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
    let expected = encode(&snapshot);
    for chunk in [1, 7, 127, 128, 129, 8192, 8193] {
        let mut reader = AccountFileArchiveReader::from_snapshot(&snapshot).unwrap();
        let mut actual = Vec::new();
        let mut scratch = vec![0; chunk];
        loop {
            let n = reader.read(&mut scratch).unwrap();
            if n == 0 {
                break;
            }
            actual.extend_from_slice(&scratch[..n]);
        }
        assert_eq!(actual, expected);
    }
    let verified = verify_account_file_archive(&expected).unwrap();
    let mut reader = AccountFileArchiveReader::from_verified(&verified).unwrap();
    let mut offsets = vec![
        0,
        1,
        127,
        128,
        expected.len() - 1,
        expected.len(),
        expected.len() + 1,
    ];
    let mut at = 128 + snapshot.manifest_bytes().len() + snapshot.accounts().bundle_bytes().len();
    offsets.extend([128 + snapshot.manifest_bytes().len() - 1, at - 1, at]);
    for file in snapshot.files() {
        offsets.extend([at + 31, at + 32, at + 47, at + 48]);
        let mut native = FileArchiveReader::from_snapshot(file).unwrap();
        at += 48 + native.seek(SeekFrom::End(0)).unwrap() as usize;
        offsets.extend([at - 1, at]);
    }
    for offset in offsets {
        reader.seek(SeekFrom::Start(offset as u64)).unwrap();
        let mut out = [0; 193];
        let n = reader.read(&mut out).unwrap();
        let start = offset.min(expected.len());
        assert_eq!(&out[..n], &expected[start..(start + n)]);
    }
    reader.seek(SeekFrom::Start(7)).unwrap();
    assert!(reader.seek(SeekFrom::Current(-8)).is_err());
    assert_eq!(reader.stream_position().unwrap(), 7);
    assert!(reader.seek(SeekFrom::End(i64::MIN)).is_err());
    assert_eq!(reader.stream_position().unwrap(), 7);
    reader.seek(SeekFrom::Start(u64::MAX)).unwrap();
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    assert_eq!(reader.read(&mut []).unwrap(), 0);
}

#[test]
fn repaired_declared_roster_bindings_frames_and_nested_corruption_are_refused() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, _, paths) = prepare(temp.path(), 2, false);
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
    let baseline = encode(&snapshot);
    let original = snapshot.manifest();
    for kind in 0..3 {
        let private = if kind == 0 {
            Vec::new()
        } else {
            original.private_projects().to_vec()
        };
        let bindings = if kind == 0 {
            Vec::new()
        } else if kind == 1 {
            original.file_stores()[..1].to_vec()
        } else {
            original
                .file_stores()
                .iter()
                .map(|b| {
                    FileRootBinding::new(
                        b.project(),
                        &if b == &original.file_stores()[0] {
                            "ffffffffffffffffffffffffffffffff".into()
                        } else {
                            b.database_id().to_string()
                        },
                    )
                    .unwrap()
                })
                .collect()
        };
        let manifest = AccountFileRootManifest::new(private, bindings, 50).unwrap();
        assert!(verify_account_file_archive(&replace_manifest(&baseline, &manifest)).is_err());
    }
    let first = 128 + snapshot.manifest_bytes().len() + snapshot.accounts().bundle_bytes().len();
    for offset in [
        first,
        first + 32,
        first + 39,
        first + 40,
        first + 47,
        first + 48,
        baseline.len() - 1,
    ] {
        let mut bytes = baseline.clone();
        bytes[offset] ^= 1;
        seal(&mut bytes);
        assert!(
            verify_account_file_archive(&bytes).is_err(),
            "offset={offset}"
        );
    }
    let mut trailing = baseline.clone();
    trailing.push(0);
    seal(&mut trailing);
    assert!(verify_account_file_archive(&trailing).is_err());
    let length = u64::from_le_bytes(baseline[first + 32..first + 40].try_into().unwrap()) as usize;
    let second = first + 48 + length;
    let mut swapped = baseline[..first].to_vec();
    swapped.extend_from_slice(&baseline[second..]);
    swapped.extend_from_slice(&baseline[first..second]);
    seal(&mut swapped);
    assert!(verify_account_file_archive(&swapped).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_valid_joint_archives_preserve_rows_files_and_random_borrowed_ranges(compact in any::<bool>(),payload in prop::collection::vec(any::<u8>(),0..8194), ranges in prop::collection::vec((any::<u64>(),0usize..257),0..17)) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let temp=tempfile::tempdir().unwrap();let(f,_,paths)=prepare(temp.path(),1,compact);let mut root=AccountRoot::open(&f.root,pool()).unwrap();let mut files=open(&paths);files[0].publish(FileId::from_bytes([6;16]),ObjectId::from_bytes([7;16]),[4;16],"model",&payload).unwrap();let snapshot=capture_account_file_components(&mut root,&mut files).unwrap();let bytes=encode(&snapshot);let verified=verify_account_file_archive(&bytes).unwrap();prop_assert_eq!(verified.files()[0].objects().objects().iter().find(|o|o.object()==ObjectId::from_bytes([7;16])).unwrap().payload(),payload.as_slice());let mut reader=AccountFileArchiveReader::from_verified(&verified).unwrap();for(raw,count)in ranges {let offset=(raw%(bytes.len()as u64+2))as usize;reader.seek(SeekFrom::Start(offset as u64)).unwrap();let mut out=vec![0;count];let n=reader.read(&mut out).unwrap();let start=offset.min(bytes.len());prop_assert_eq!(&out[..n],&bytes[start..start+n]);}
    }
}

#[test]
fn untrusted_joined_physical_inventory_admits_128_and_refuses_a_valid_129_object_pair() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, _, paths) = prepare(temp.path(), 2, true);
    for path in &paths {
        let project = path.file_name().unwrap().to_str().unwrap().parse().unwrap();
        let mut objects = ProjectDirectory::open(path.join("objects"), project).unwrap();
        for n in 10..72 {
            objects.put(ObjectId::from_bytes([n; 16]), b"").unwrap();
        }
    }
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
    let baseline = encode(&snapshot);
    assert_eq!(
        verify_account_file_archive(&baseline)
            .unwrap()
            .physical_objects(),
        128
    );
    drop(files);
    let project = paths[1]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    ProjectDirectory::open(paths[1].join("objects"), project)
        .unwrap()
        .put(ObjectId::from_bytes([80; 16]), b"")
        .unwrap();
    let mut files = open(&paths);
    let second = files[1].capture().unwrap();
    assert_eq!(second.objects().objects().len(), 65);
    let mut native = Vec::new();
    FileArchiveReader::from_snapshot(&second)
        .unwrap()
        .read_to_end(&mut native)
        .unwrap();
    emilybase_files::verify_file_archive(&native, project).unwrap();
    let first = 128 + snapshot.manifest_bytes().len() + snapshot.accounts().bundle_bytes().len();
    let length = u64::from_le_bytes(baseline[first + 32..first + 40].try_into().unwrap()) as usize;
    let second_at = first + 48 + length;
    let mut oversized = baseline[..second_at + 48].to_vec();
    oversized[second_at + 32..second_at + 40].copy_from_slice(&(native.len() as u64).to_le_bytes());
    oversized.extend_from_slice(&native);
    seal(&mut oversized);
    assert!(matches!(
        verify_account_file_archive(&oversized),
        Err(Error::Limit)
    ));
    files[0].inspect().unwrap();
    files[1].inspect().unwrap();
}

#[test]
#[ignore = "opt-in synthetic archive fuzz corpus generator"]
fn generate_account_file_archive_corpus() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let output = PathBuf::from(
        std::env::var_os("EMILYBASE_COMPONENT_ARCHIVE_CORPUS").expect("corpus output directory"),
    );
    fs::create_dir_all(&output).unwrap();
    for count in [0, 1, 2, 4] {
        for compact in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (f, _, paths) = prepare(temp.path(), count, compact);
            let mut root = AccountRoot::open(&f.root, pool()).unwrap();
            let mut files = open(&paths);
            let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
            let bytes = encode(&snapshot);
            verify_account_file_archive(&bytes).unwrap();
            fs::write(output.join(format!("native-{count}-{compact}")), bytes).unwrap();
        }
    }
}
