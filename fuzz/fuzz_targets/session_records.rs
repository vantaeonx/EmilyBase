#![no_main]
use emilybase_auth::accounts::inspect_session_record;
use emilybase_catalog::Value;
use libfuzzer_sys::fuzz_target;

const PROJECT: &str = "11111111111111111111111111111111";
fn digest(kind: u8) -> Vec<u8> {
    let mut bytes = vec![0; 92];
    bytes[..8].copy_from_slice(b"EBSK\0\0\0\0");
    bytes[8..12].copy_from_slice(&[1, 0, kind, 0]);
    bytes[12..28].fill(0x11);
    bytes[28..44].fill(9);
    bytes[44..60].fill(3);
    bytes
}
fn verifier(bytes: &[u8], kind: u8, family: &[u8], incarnation: &[u8]) -> bool {
    bytes.len() == 92
        && bytes[..8] == *b"EBSK\0\0\0\0"
        && bytes[8..12] == [1, 0, kind, 0]
        && bytes[12..28] == [0x11; 16]
        && bytes[28..44] == *incarnation
        && bytes[44..60] == *family
}
fn model(row: &[Value]) -> bool {
    let [
        Value::Text(family),
        Value::Bytes(incarnation),
        Value::Text(login),
        Value::Bytes(user),
        Value::Integer(epoch),
        Value::Integer(generation),
        Value::Integer(created),
        Value::Integer(issued),
        Value::Integer(access),
        Value::Integer(refresh),
        Value::Integer(absolute),
        Value::Bytes(a),
        Value::Bytes(r),
        Value::Boolean(_),
    ] = row
    else {
        return false;
    };
    let hex_ok = family.len() == 32
        && family
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let login_ok = !login.is_empty()
        && login.len() <= 64
        && login.as_bytes()[0].is_ascii_alphanumeric()
        && login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
    if !hex_ok
        || !login_ok
        || incarnation.len() != 16
        || user.len() != 16
        || *epoch <= 0
        || *generation <= 0
        || *created < 0
        || *issued < *created
        || created.checked_add(2592000) != Some(*absolute)
        || *issued >= *absolute
    {
        return false;
    }
    let remaining = absolute - issued;
    if *access != issued + remaining.min(900) || *refresh != issued + remaining.min(604800) {
        return false;
    }
    let family = (0..16)
        .map(|i| u8::from_str_radix(&family[2 * i..2 * i + 2], 16).unwrap())
        .collect::<Vec<_>>();
    verifier(a, 1, &family, incarnation) && verifier(r, 2, &family, incarnation)
}

fuzz_target!(|data: &[u8]| {
    // Bounded pure private-record inspection, never a token/login/session service.
    let data = &data[..data.len().min(512)];
    let mut row = vec![
        Value::Text("03".repeat(16)),
        Value::Bytes(vec![9; 16]),
        Value::Text("synthetic".into()),
        Value::Bytes(vec![7; 16]),
        Value::Integer(1),
        Value::Integer(1),
        Value::Integer(0),
        Value::Integer(0),
        Value::Integer(900),
        Value::Integer(604800),
        Value::Integer(2592000),
        Value::Bytes(digest(1)),
        Value::Bytes(digest(2)),
        Value::Boolean(false),
    ];
    if data.len() >= 2 {
        let index = usize::from(data[0]) % 14;
        match data[1] % 7 {
            0 => row[index] = Value::Null,
            1 => {
                let mut bytes = [0; 8];
                for (i, b) in data.iter().skip(2).take(8).enumerate() {
                    bytes[i] = *b;
                }
                row[index] = Value::Integer(i64::from_le_bytes(bytes));
            }
            2 => row[index] = Value::Bytes(data.iter().skip(2).take(192).copied().collect()),
            3 => {
                row[index] =
                    Value::Text(String::from_utf8_lossy(&data[2..data.len().min(130)]).into())
            }
            4 => row[index] = Value::Boolean(data[0] & 1 != 0),
            5 => row.truncate(index),
            _ => row.push(Value::Integer(1)),
        }
    }
    assert_eq!(inspect_session_record(&row, PROJECT).is_ok(), model(&row));
    // Also exercise valid clipping across the complete signed time range.
    let mut bytes = [0; 8];
    for (i, b) in data.iter().take(8).enumerate() {
        bytes[i] = *b;
    }
    let created = (u64::from_le_bytes(bytes) % ((i64::MAX - 2592000) as u64 + 1)) as i64;
    let elapsed = i64::from(data.first().copied().unwrap_or(0)) * 10000;
    let issued = created + elapsed;
    let absolute = created + 2592000;
    let mut valid = vec![
        Value::Text("03".repeat(16)),
        Value::Bytes(vec![9; 16]),
        Value::Text("synthetic".into()),
        Value::Bytes(vec![7; 16]),
        Value::Integer(1),
        Value::Integer(1),
        Value::Integer(created),
        Value::Integer(issued),
        Value::Integer(issued + (absolute - issued).min(900)),
        Value::Integer(issued + (absolute - issued).min(604800)),
        Value::Integer(absolute),
        Value::Bytes(digest(1)),
        Value::Bytes(digest(2)),
        Value::Boolean(false),
    ];
    assert!(inspect_session_record(&valid, PROJECT).is_ok());
    valid[8] = Value::Integer(issued);
    assert!(inspect_session_record(&valid, PROJECT).is_err());
});
