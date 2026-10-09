#![no_main]
use emilybase_object_storage::{
    Error, HEADER_BYTES, MAX_PAYLOAD_BYTES, ObjectId, ProjectId, encode, verify, verify_stream,
};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fn check(bytes: &[u8], project: ProjectId, object: ObjectId) {
    let whole = verify(bytes, project, object);
    let stream = verify_stream(&mut Cursor::new(bytes), bytes.len(), project, object);
    match (&whole, &stream) {
        (Ok(view), Ok(report)) => {
            assert_eq!(view.payload().len(), report.payload_bytes);
            assert_eq!(view.sha256(), &report.sha256);
        }
        (Err(Error::Version(a)), Err(Error::Version(b))) => assert_eq!(a, b),
        (Err(a), Err(b)) => assert_eq!(std::mem::discriminant(a), std::mem::discriminant(b)),
        _ => panic!("byte/reader admission mismatch"),
    }
    if let Ok(view) = whole {
        assert!(view.payload().len() <= MAX_PAYLOAD_BYTES);
        assert_eq!(view.project(), project);
        assert_eq!(view.object(), object);
        assert_eq!(encode(project, object, view.payload()).unwrap(), bytes);
    }
}

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
    let bytes = &data[32..];
    check(bytes, project, object);
    if (HEADER_BYTES..=HEADER_BYTES + MAX_PAYLOAD_BYTES).contains(&bytes.len()) {
        let mut repaired = bytes.to_vec();
        let crc = crc32fast::hash(&repaired[..92]);
        repaired[92..96].copy_from_slice(&crc.to_le_bytes());
        check(&repaired, project, object);
    }
    // Every input also reaches complete payload hashing, independent of whether
    // mutations happened to preserve an existing header checksum and length.
    if bytes.len() <= MAX_PAYLOAD_BYTES {
        let canonical = encode(project, object, bytes).unwrap();
        check(&canonical, project, object);
        for declared in [canonical.len() - 1, canonical.len() + 1] {
            assert!(
                verify_stream(&mut Cursor::new(&canonical), declared, project, object).is_err()
            );
        }
    }
});
