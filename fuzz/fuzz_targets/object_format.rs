#![no_main]
use emilybase_object_storage::{MAX_PAYLOAD_BYTES, ObjectId, ProjectId, encode, verify};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 32 {
        return;
    }
    let mut project = [0; 16];
    let mut object = [0; 16];
    project.copy_from_slice(&data[..16]);
    object.copy_from_slice(&data[16..32]);
    let project = ProjectId::from_bytes(project);
    let object = ObjectId::from_bytes(object);
    if let Ok(view) = verify(&data[32..], project, object) {
        assert!(view.payload().len() <= MAX_PAYLOAD_BYTES);
        assert_eq!(view.project(), project);
        assert_eq!(view.object(), object);
        assert_eq!(
            encode(project, object, view.payload()).unwrap(),
            &data[32..]
        );
    }
});
