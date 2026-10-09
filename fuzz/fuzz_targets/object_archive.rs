#![no_main]
use emilybase_object_storage::{
    MAX_INVENTORY_BYTES, MAX_INVENTORY_OBJECTS, ProjectId, encode_verified_archive, verify_archive,
};
use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};

fn check(bytes: &[u8], project: ProjectId) {
    if let Ok(view) = verify_archive(bytes, project) {
        assert!(view.objects().len() <= MAX_INVENTORY_OBJECTS);
        assert!(view.payload_bytes() <= MAX_INVENTORY_BYTES);
        assert_eq!(view.project(), project);
        assert_eq!(encode_verified_archive(&view).unwrap(), bytes);
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
