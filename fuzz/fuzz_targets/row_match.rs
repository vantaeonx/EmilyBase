#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Value, decode_row, encode_row, row_matches};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|bytes: &[u8]| {
    if bytes.len() > 4096 {
        return;
    }
    let expected = vec![
        Value::Integer(i64::from(bytes.first().copied().unwrap_or(0))),
        Value::Boolean(bytes.get(1).is_some_and(|b| b & 1 != 0)),
        Value::Null,
    ];
    match decode_row(bytes) {
        Ok(row) => {
            assert_eq!(row_matches(bytes, &expected).unwrap(), row == expected);
            assert!(row_matches(bytes, &row).unwrap());
            let mut other = row;
            other.push(Value::Null);
            assert!(!row_matches(bytes, &other).unwrap());
        }
        Err(error) => assert_eq!(
            row_matches(bytes, &expected).unwrap_err().to_string(),
            error.to_string()
        ),
    }
    let count = bytes.len().min(3072);
    let text = std::str::from_utf8(&bytes[..count]).unwrap_or("я\0");
    let row = vec![
        Value::Integer(i64::from(bytes.first().copied().unwrap_or(0))),
        Value::Text(text.into()),
        Value::Boolean(bytes.get(1).is_some_and(|b| b & 1 != 0)),
        Value::Float(-0.0),
        Value::Null,
    ];
    let Ok(encoded) = encode_row(&row) else {
        return;
    };
    assert!(row_matches(&encoded, &row).unwrap());
    let mut changed = row.clone();
    changed[0] = Value::Integer(-1);
    assert!(!row_matches(&encoded, &changed).unwrap());
    let mut mutated = encoded;
    if let Some(replacement) = bytes.last() {
        let offset = usize::from(bytes.first().copied().unwrap_or(0)) % mutated.len();
        mutated[offset] = *replacement;
        match decode_row(&mutated) {
            Ok(decoded) => assert_eq!(row_matches(&mutated, &row).unwrap(), decoded == row),
            Err(error) => assert_eq!(
                row_matches(&mutated, &row).unwrap_err().to_string(),
                error.to_string()
            ),
        }
    }
});
