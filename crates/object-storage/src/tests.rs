use super::*;
use proptest::prelude::*;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};

const PROJECT: ProjectId = ProjectId::from_bytes([1; 16]);
const OBJECT: ObjectId = ObjectId::from_bytes([2; 16]);

#[test]
fn native_object_publication_is_private_checked_and_never_overwrites_existing_names() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.object");
    let report = publish_file(&path, PROJECT, OBJECT, b"synthetic-object").unwrap();
    assert_eq!(report, inspect_file(&path, PROJECT, OBJECT).unwrap());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::metadata(&path).unwrap().nlink(), 1);
    let before = fs::read(&path).unwrap();
    assert!(publish_file(&path, PROJECT, OBJECT, b"replacement").is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let alias = d.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(publish_file(&alias, PROJECT, OBJECT, b"replacement").is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 2);
}

#[test]
fn concurrent_native_publication_selects_one_complete_object_without_mixed_bytes() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.object");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let a = barrier.clone();
        let p = &path;
        let first = scope.spawn(move || {
            a.wait();
            publish_file(p, PROJECT, OBJECT, &[0x11; 65536])
        });
        let a = barrier.clone();
        let p = &path;
        let second = scope.spawn(move || {
            a.wait();
            publish_file(p, PROJECT, OBJECT, &[0x22; 65536])
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let bytes = fs::read(&path).unwrap();
    let view = verify(&bytes, PROJECT, OBJECT).unwrap();
    assert!(view.payload() == [0x11; 65536] || view.payload() == [0x22; 65536]);
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
}
fn seal(bytes: &mut [u8]) {
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
fn independent_python_struct_hashlib_zlib_vector_matches_exact_wire_bytes() {
    let project = ProjectId::from_bytes(std::array::from_fn(|i| i as u8));
    let object = ObjectId::from_bytes(std::array::from_fn(|i| 240 + i as u8));
    let payload = "synthetic-object\0界".as_bytes();
    // Independently produced with Python struct, hashlib.sha256 and zlib.crc32.
    let expected = "454d494c594f424a0100000060000000000102030405060708090a0b0c0d0e0ff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff1400000000000000446c0be5cfad1f2ae34522edaa239483d3333996d4a5ce5db151cc7299c70de400000000d342f96c73796e7468657469632d6f626a65637400e7958c";
    let bytes = encode(project, object, payload).unwrap();
    assert_eq!(
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        expected
    );
    let checked = verify(&bytes, project, object).unwrap();
    assert_eq!(checked.payload(), payload);
    assert_eq!(checked.project(), project);
    assert_eq!(checked.object(), object);
    assert_eq!(checked.payload().as_ptr(), bytes[HEADER_BYTES..].as_ptr());
    assert!(!format!("{checked:?}").contains("synthetic-object"));
}

#[test]
fn identity_types_accept_only_full_lowercase_hex_and_preserve_every_byte() {
    for invalid in [
        "",
        "../private",
        "0",
        "0x00000000000000000000000000000000",
        "00000000000000000000000000000000\n",
        "A0000000000000000000000000000000",
        "界界界界界界界界界界界",
    ] {
        assert!(invalid.parse::<ProjectId>().is_err());
        assert!(invalid.parse::<ObjectId>().is_err());
    }
    let all = ProjectId::from_bytes(std::array::from_fn(|i| i as u8 * 17));
    assert_eq!(all.to_string(), "00112233445566778899aabbccddeeff");
    assert_eq!(all.to_string().parse::<ProjectId>().unwrap(), all);
    assert_eq!(
        "00".repeat(16).parse::<ObjectId>().unwrap().as_bytes(),
        &[0; 16]
    );
}

#[test]
fn exact_empty_and_maximum_payload_bounds_round_trip_without_accepting_extra_bytes() {
    for payload in [Vec::new(), vec![0xa5; MAX_PAYLOAD_BYTES]] {
        let bytes = encode(PROJECT, OBJECT, &payload).unwrap();
        assert_eq!(verify(&bytes, PROJECT, OBJECT).unwrap().payload(), payload);
        let mut extra = bytes;
        extra.push(0);
        assert!(verify(&extra, PROJECT, OBJECT).is_err());
    }
    assert!(matches!(
        encode(PROJECT, OBJECT, &vec![0; MAX_PAYLOAD_BYTES + 1]),
        Err(Error::Limit)
    ));
    assert!(matches!(
        verify(
            &vec![0; HEADER_BYTES + MAX_PAYLOAD_BYTES + 1],
            PROJECT,
            OBJECT
        ),
        Err(Error::Limit)
    ));
}

#[test]
fn every_truncated_prefix_and_every_single_byte_header_or_payload_corruption_refuses() {
    let original = encode(PROJECT, OBJECT, &[0x42; 128]).unwrap();
    for end in 0..original.len() {
        assert!(
            verify(&original[..end], PROJECT, OBJECT).is_err(),
            "prefix {end}"
        );
    }
    for byte in 0..original.len() {
        let mut damaged = original.clone();
        damaged[byte] ^= 1;
        assert!(
            verify(&damaged, PROJECT, OBJECT).is_err(),
            "damaged byte {byte}"
        );
    }
}

#[test]
fn valid_crc_does_not_hide_unknown_versions_flags_scope_lengths_or_wrong_payload_digest() {
    let bytes = encode(PROJECT, OBJECT, b"synthetic-body").unwrap();
    for (offset, value) in [(0, b'X'), (10, 1), (12, 95), (88, 1)] {
        let mut changed = bytes.clone();
        changed[offset] = value;
        seal(&mut changed);
        assert!(matches!(
            verify(&changed, PROJECT, OBJECT),
            Err(Error::Format)
        ));
    }
    let mut changed = bytes.clone();
    changed[8..10].copy_from_slice(&2u16.to_le_bytes());
    seal(&mut changed);
    assert!(matches!(
        verify(&changed, PROJECT, OBJECT),
        Err(Error::Version(2))
    ));
    for offset in [16, 32] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        seal(&mut changed);
        assert!(matches!(
            verify(&changed, PROJECT, OBJECT),
            Err(Error::Scope)
        ));
    }
    for length in [u64::MAX, MAX_PAYLOAD_BYTES as u64 + 1] {
        let mut changed = bytes.clone();
        changed[48..56].copy_from_slice(&length.to_le_bytes());
        seal(&mut changed);
        assert!(matches!(
            verify(&changed, PROJECT, OBJECT),
            Err(Error::Limit)
        ));
    }
    let mut changed = bytes.clone();
    changed[48..56].copy_from_slice(&1u64.to_le_bytes());
    seal(&mut changed);
    assert!(matches!(
        verify(&changed, PROJECT, OBJECT),
        Err(Error::Format)
    ));
    let mut changed = bytes;
    changed[56] ^= 1;
    seal(&mut changed);
    assert!(matches!(
        verify(&changed, PROJECT, OBJECT),
        Err(Error::PayloadChecksum)
    ));
}

#[test]
fn expected_scope_is_required_and_checksums_are_not_a_permission_or_authenticity_proof() {
    let a = encode(PROJECT, OBJECT, b"synthetic-a").unwrap();
    assert!(matches!(
        verify(&a, ProjectId::from_bytes([3; 16]), OBJECT),
        Err(Error::Scope)
    ));
    assert!(matches!(
        verify(&a, PROJECT, ObjectId::from_bytes([3; 16])),
        Err(Error::Scope)
    ));
    let b = encode(PROJECT, OBJECT, b"synthetic-b").unwrap();
    assert_eq!(
        verify(&b, PROJECT, OBJECT).unwrap().payload(),
        b"synthetic-b"
    );
    assert_ne!(
        verify(&a, PROJECT, OBJECT).unwrap().sha256(),
        verify(&b, PROJECT, OBJECT).unwrap().sha256()
    );
}

#[test]
fn offline_private_inspection_is_readonly_and_reports_no_payload() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.blob");
    let bytes = encode(PROJECT, OBJECT, b"synthetic-sensitive-body").unwrap();
    private(&path, &bytes);
    let report = inspect_file(&path, PROJECT, OBJECT).unwrap();
    assert_eq!(report.payload_bytes, 24);
    assert_eq!(
        &report.sha256,
        verify(&bytes, PROJECT, OBJECT).unwrap().sha256()
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!format!("{report:?}").contains("synthetic-sensitive"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(inspect_file(&path, PROJECT, OBJECT).unwrap(), report);
}

#[test]
fn offline_file_inspection_rejects_aliases_nonregular_public_corrupt_and_oversized_inputs() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("synthetic.blob");
    let bytes = encode(PROJECT, OBJECT, b"synthetic").unwrap();
    private(&path, &bytes);
    let alias = d.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(inspect_file(&alias, PROJECT, OBJECT).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    assert!(inspect_file(&path, PROJECT, OBJECT).is_err());
    fs::remove_file(&alias).unwrap();
    for mode in [0o644, 0o660, 0o700, 0o200] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        assert!(inspect_file(&path, PROJECT, OBJECT).is_err());
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(inspect_file(d.path(), PROJECT, OBJECT).is_err());
    let fifo = d.path().join("fifo");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(inspect_file(&fifo, PROJECT, OBJECT).is_err());
    let file = File::options().write(true).open(&path).unwrap();
    file.set_len((HEADER_BYTES + MAX_PAYLOAD_BYTES + 1) as u64)
        .unwrap();
    assert!(matches!(
        inspect_file(&path, PROJECT, OBJECT),
        Err(Error::Limit)
    ));
    file.set_len(2).unwrap();
    assert!(inspect_file(&path, PROJECT, OBJECT).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn arbitrary_payload_scope_and_single_mutation_follow_independent_byte_model(
        project in any::<[u8;16]>(),object in any::<[u8;16]>(),
        payload in prop::collection::vec(any::<u8>(),0..8193),index in any::<usize>()
    ) {
        let project=ProjectId::from_bytes(project);let object=ObjectId::from_bytes(object);
        let bytes=encode(project,object,&payload).unwrap();let view=verify(&bytes,project,object).unwrap();
        prop_assert_eq!(view.payload(),&payload);prop_assert_eq!(&bytes[16..32],project.as_bytes());
        prop_assert_eq!(&bytes[32..48],object.as_bytes());prop_assert_eq!(&bytes[HEADER_BYTES..],&payload);
        let encoded=encode(view.project(),view.object(),view.payload()).unwrap();
        prop_assert_eq!(encoded.as_slice(),bytes.as_slice());
        let mut damaged=bytes.clone();let index=index%damaged.len();damaged[index]^=1;
        prop_assert!(verify(&damaged,project,object).is_err());
        let mut foreign=*project.as_bytes();foreign[0]^=1;
        prop_assert!(matches!(verify(&bytes,ProjectId::from_bytes(foreign),object),Err(Error::Scope)));
    }
    #[test]
    fn arbitrary_untrusted_inputs_never_construct_invalid_verified_views(bytes in prop::collection::vec(any::<u8>(),0..2049)) {
        if let Ok(view)=verify(&bytes,PROJECT,OBJECT) {
            prop_assert!(view.payload().len()<=MAX_PAYLOAD_BYTES);
            prop_assert_eq!(encode(PROJECT,OBJECT,view.payload()).unwrap(),bytes);
        }
    }
}
