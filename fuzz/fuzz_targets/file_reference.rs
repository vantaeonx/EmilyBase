#![no_main]
use emilybase_catalog::Value;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if input.len() < 8 || input.len() > 4008 {
        return;
    }
    let last = u64::from_le_bytes(input[..8].try_into().unwrap());
    if let Ok(row) = emilybase_catalog::decode_row(&input[8..]) {
        let accepted = match row.as_slice() {
            [
                Value::Text(id),
                Value::Bytes(object),
                Value::Bytes(owner),
                Value::Text(name),
                Value::Integer(bytes),
                Value::Bytes(hash),
                Value::Bytes(revision),
            ] => {
                let valid_revision = revision.len() == 8 && {
                    let number = u64::from_le_bytes(revision.as_slice().try_into().unwrap());
                    number > 0 && number <= last
                };
                id.len() == 32
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && object.len() == 16
                    && owner.len() == 16
                    && hash.len() == 32
                    && !name.is_empty()
                    && name.len() <= 256
                    && !name.chars().any(char::is_control)
                    && *bytes >= 0
                    && *bytes <= 8 * 1024 * 1024
                    && valid_revision
            }
            _ => false,
        };
        let actual = emilybase_files::inspect_file_record(&row, last);
        assert_eq!(actual.is_ok(), accepted);
        if let Ok(info) = actual {
            let canonical = vec![
                Value::Text(info.id().to_string()),
                Value::Bytes(info.object().as_bytes().to_vec()),
                Value::Bytes(info.owner().to_vec()),
                Value::Text(info.name().into()),
                Value::Integer(info.report().payload_bytes as i64),
                Value::Bytes(info.report().sha256.to_vec()),
                Value::Bytes(info.revision().to_le_bytes().to_vec()),
            ];
            assert_eq!(row, canonical);
            assert!(info.revision() > 0 && info.revision() <= last);
            assert!(info.report().payload_bytes <= emilybase_object_storage::MAX_PAYLOAD_BYTES);
            assert!(info.name().len() <= emilybase_files::MAX_FILE_NAME_BYTES);
            assert!(info.name().chars().all(|c| !c.is_control()));
        }
    }
});
