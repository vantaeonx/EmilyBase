#![no_main]
use emilybase_files::{FileArchiveReader, verify_file_archive};
use emilybase_object_storage::ProjectId;
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
use std::io::Read;

// One selector byte, followed by at most256 KiB of paired archive. Mode1 repairs
// outer digests/CRC only, allowing nested corruption to reach original decoders.
// Lengths, magic, version, flags, identities and nested checksums are never fixed.
fuzz_target!(|input: &[u8]| {
    let Some((&mode, input)) = input.split_first() else {
        return;
    };
    if input.len() > 262_144 {
        return;
    }
    let mut repaired;
    let bytes = if mode & 1 == 1 && input.len() >= 192 {
        let declared = u64::from_le_bytes(input[56..64].try_into().unwrap());
        if declared > (input.len() - 192) as u64 {
            return;
        }
        let middle = 192 + declared as usize;
        repaired = input.to_vec();
        let metadata: [u8; 32] = Sha256::digest(&repaired[192..middle]).into();
        let objects: [u8; 32] = Sha256::digest(&repaired[middle..]).into();
        repaired[72..104].copy_from_slice(&metadata);
        repaired[104..136].copy_from_slice(&objects);
        let crc = crc32fast::hash(&repaired[..188]);
        repaired[188..192].copy_from_slice(&crc.to_le_bytes());
        repaired.as_slice()
    } else {
        input
    };
    if let Ok(view) = verify_file_archive(bytes, ProjectId::from_bytes([1; 16])) {
        let objects = view.objects().objects();
        assert!(objects.len() <= view.quota().objects());
        assert!(view.objects().payload_bytes() <= view.quota().payload_bytes());
        assert_eq!(
            objects
                .iter()
                .map(|o| o.payload().len() as u64)
                .sum::<u64>(),
            view.objects().payload_bytes()
        );
        for file in view.files() {
            let object = objects
                .iter()
                .find(|o| o.object() == file.object())
                .unwrap();
            assert_eq!(file.report().payload_bytes, object.payload().len());
            assert_eq!(&file.report().sha256, object.sha256());
            assert!(
                file.revision() > 0 && file.revision() <= view.metadata_report().last_transaction
            );
        }
        let mut reader = FileArchiveReader::from_verified(&view).unwrap();
        assert_eq!(reader.encoded_bytes(), bytes.len());
        let mut canonical = Vec::new();
        reader.read_to_end(&mut canonical).unwrap();
        assert_eq!(canonical, bytes);
    }
});
