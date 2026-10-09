#![no_main]
use emilybase_object_storage::object_id_from_name;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|name: &[u8]| {
    if let Ok(object) = object_id_from_name(name) {
        assert_eq!(name.len(), 39);
        assert_eq!(format!("{object}.object").as_bytes(), name);
        assert!(name[..32].iter().all(u8::is_ascii_hexdigit));
        assert!(name[..32].iter().all(|b| !b.is_ascii_uppercase()));
    }
});
