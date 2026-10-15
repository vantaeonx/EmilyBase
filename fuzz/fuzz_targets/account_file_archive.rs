#![no_main]
#![forbid(unsafe_code)]
use emilybase_server::{AccountFileArchiveReader, verify_account_file_archive};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};

fn check(bytes: &[u8]) {
    let Ok(verified) = verify_account_file_archive(bytes) else {
        return;
    };
    assert_eq!(verified.encoded_bytes(), bytes.len());
    assert!(verified.physical_objects() <= 128);
    assert!(verified.copied_bytes() <= 128 * 1024 * 1024);
    assert!(verified.files().len() <= 4);
    let mut reader = AccountFileArchiveReader::from_verified(&verified).unwrap();
    assert_eq!(reader.encoded_bytes(), bytes.len());
    let mut scratch = [0; 8192];
    let mut at = 0;
    loop {
        let n = reader.read(&mut scratch).unwrap();
        if n == 0 {
            break;
        }
        assert_eq!(&scratch[..n], &bytes[at..at + n]);
        at += n;
    }
    assert_eq!(at, bytes.len());
    for offset in [0, 127, 128, bytes.len() - 1, bytes.len(), bytes.len() + 1] {
        reader.seek(SeekFrom::Start(offset as u64)).unwrap();
        let n = reader.read(&mut scratch[..193]).unwrap();
        let start = offset.min(bytes.len());
        assert_eq!(&scratch[..n], &bytes[start..start + n]);
    }
}
fuzz_target!(|bytes: &[u8]| {
    check(bytes);
    if !(128..=1048576).contains(&bytes.len()) {
        return;
    }
    let mut repaired = bytes.to_vec();
    repaired[32..40].copy_from_slice(&((bytes.len() - 128) as u64).to_le_bytes());
    let hash = Sha256::digest(&repaired[128..]);
    repaired[40..72].copy_from_slice(&hash);
    let crc = crc32fast::hash(&repaired[..124]);
    repaired[124..128].copy_from_slice(&crc.to_le_bytes());
    check(&repaired);
});
