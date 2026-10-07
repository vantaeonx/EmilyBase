#![no_main]
use emilybase_auth::accounts::inspect_account_record;
use emilybase_catalog::Value;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Private record inspection only; no password KDF, I/O or account provisioning.
    let mut digest = vec![0; 72];
    digest[..8].copy_from_slice(b"EBPWD\0\0\0");
    digest[8..12].copy_from_slice(&[1, 0, 2, 19]);
    digest[12..16].copy_from_slice(&19456_u32.to_le_bytes());
    digest[16..20].copy_from_slice(&2_u32.to_le_bytes());
    digest[20..24].copy_from_slice(&1_u32.to_le_bytes());
    for (i, byte) in data.iter().take(48).enumerate() {
        digest[24 + i] = *byte;
    }
    let mut row = vec![
        Value::Text("synthetic".into()),
        Value::Bytes(vec![7; 16]),
        Value::Bytes(digest),
        Value::Integer(1),
        Value::Boolean(false),
    ];
    if let Some(tag) = data.first() {
        match tag % 8 {
            0 => {
                row[0] = Value::Text(String::from_utf8_lossy(&data[1..data.len().min(130)]).into())
            }
            1 => row[1] = Value::Bytes(data.iter().skip(1).take(32).copied().collect()),
            2 => row[2] = Value::Bytes(data.iter().skip(1).take(128).copied().collect()),
            3 => {
                let mut bytes = [0; 8];
                for (i, b) in data.iter().skip(1).take(8).enumerate() {
                    bytes[i] = *b;
                }
                row[3] = Value::Integer(i64::from_le_bytes(bytes));
            }
            4 => row[4] = Value::Boolean(tag & 8 != 0),
            5 => row[usize::from(*tag) % 5] = Value::Null,
            6 => {
                row.pop();
            }
            _ => row.push(Value::Integer(1)),
        }
    }
    let accepted = match row.as_slice() {
        [
            Value::Text(login),
            Value::Bytes(id),
            Value::Bytes(digest),
            Value::Integer(epoch),
            Value::Boolean(_),
        ] => {
            let name_ok = !login.is_empty()
                && login.len() <= 64
                && login
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && login
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
            name_ok
                && id.len() == 16
                && *epoch > 0
                && digest.len() == 72
                && digest[..8] == *b"EBPWD\0\0\0"
                && digest[8..12] == [1, 0, 2, 19]
                && digest[12..16] == 19456_u32.to_le_bytes()
                && digest[16..20] == 2_u32.to_le_bytes()
                && digest[20..24] == 1_u32.to_le_bytes()
        }
        _ => false,
    };
    let actual = inspect_account_record(&row);
    assert_eq!(actual.is_ok(), accepted);
    if let Ok(info) = actual {
        assert_eq!(row[0], Value::Text(info.login));
        assert_eq!(row[1], Value::Bytes(info.id.to_vec()));
        assert_eq!(row[3], Value::Integer(info.credential_epoch as i64));
        assert_eq!(row[4], Value::Boolean(info.disabled));
    }
});
