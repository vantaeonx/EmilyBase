#![no_main]
#![forbid(unsafe_code)]
use emilybase_object_storage::{
    ArchiveReader, Error, MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, ProjectId,
    encode_verified_archive, verify_archive, verify_archive_reader,
};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Seek, SeekFrom};

fn check(bytes: &[u8], project: ProjectId) {
    match (
        verify_archive(bytes, project),
        verify_archive_reader(&mut Cursor::new(bytes), bytes.len(), project),
    ) {
        (Ok(view), Ok(report)) => {
            assert!(view.objects().len() <= MAX_INVENTORY_OBJECTS);
            assert!(view.payload_bytes() <= MAX_INVENTORY_BYTES);
            assert_eq!(view.project(), project);
            assert_eq!(view.objects().len(), report.objects);
            assert_eq!(view.payload_bytes(), report.payload_bytes);
            assert_eq!(view.digest(), &report.digest);
            assert_eq!(encode_verified_archive(&view).unwrap(), bytes);
            let mut encoded = ArchiveReader::from_verified(&view).unwrap();
            assert_eq!(encoded.encoded_bytes(), bytes.len());
            assert_eq!(encoded.report(), &report);
            let mut scratch = [0; 8192];
            let mut offset = 0;
            loop {
                let count = encoded.read(&mut scratch).unwrap();
                if count == 0 {
                    break;
                }
                assert_eq!(&scratch[..count], &bytes[offset..offset + count]);
                offset += count;
            }
            assert_eq!(offset, bytes.len());
            for position in [
                0,
                127,
                128,
                bytes.len() / 2,
                bytes.len() - 1,
                bytes.len(),
                bytes.len() + 1,
            ] {
                encoded.seek(SeekFrom::Start(position as u64)).unwrap();
                let count = encoded.read(&mut scratch).unwrap();
                let start = position.min(bytes.len());
                assert_eq!(count, (bytes.len() - start).min(scratch.len()));
                assert_eq!(&scratch[..count], &bytes[start..start + count]);
            }
        }
        (Err(Error::ArchiveVersion(a)), Err(Error::ArchiveVersion(b))) => assert_eq!(a, b),
        (Err(Error::Version(a)), Err(Error::Version(b))) => assert_eq!(a, b),
        (Err(a), Err(b)) => assert_eq!(std::mem::discriminant(&a), std::mem::discriminant(&b)),
        _ => panic!("byte and seekable archive admission differ"),
    }
}
fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }
    let mut project = [0; 16];
    project.copy_from_slice(&data[..16]);
    let project = ProjectId::from_bytes(project);
    let bytes = &data[16..];
    check(bytes, project);
    // Exercise structural validation beyond outer integrity rejection too.
    // This creates untrusted checksums, not a proof or an authorization bypass.
    if (128..=262128).contains(&bytes.len()) {
        let mut resealed = bytes.to_vec();
        let body_hash = Sha256::digest(&resealed[128..]);
        resealed[88..120].copy_from_slice(&body_hash);
        let crc = crc32fast::hash(&resealed[..124]);
        resealed[124..128].copy_from_slice(&crc.to_le_bytes());
        check(&resealed, project);
    }
});
