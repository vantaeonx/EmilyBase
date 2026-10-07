use emilybase_catalog::{
    Error, MAX_ENCODED_BYTES, MAX_VALUE_BYTES, Value, decode_row, encode_row, row_matches,
};
use proptest::prelude::*;
fn error(bytes: &[u8], expected: &[Value]) -> String {
    row_matches(bytes, expected).unwrap_err().to_string()
}
#[test]
fn all_types_empty_rows_signed_zero_and_exact_payload_boundaries_match_owned_semantics() {
    let row = vec![
        Value::Null,
        Value::Boolean(true),
        Value::Integer(i64::MIN),
        Value::Float(-0.0),
        Value::Text("я\0λ".into()),
        Value::Bytes(vec![0, 255]),
    ];
    let bytes = encode_row(&row).unwrap();
    assert!(row_matches(&bytes, &row).unwrap());
    let mut other = row.clone();
    other[3] = Value::Float(0.0);
    assert!(row_matches(&bytes, &other).unwrap());
    for index in 0..row.len() {
        let mut other = row.clone();
        other[index] = Value::Integer(1);
        assert!(!row_matches(&bytes, &other).unwrap());
    }
    assert!(!row_matches(&bytes, &row[..row.len() - 1]).unwrap());
    assert!(row_matches(&encode_row(&[]).unwrap(), &[]).unwrap());
    for value in [
        Value::Text("я".repeat(MAX_VALUE_BYTES / 2)),
        Value::Bytes(vec![255; MAX_VALUE_BYTES]),
    ] {
        let bytes = encode_row(std::slice::from_ref(&value)).unwrap();
        assert!(row_matches(&bytes, &[value]).unwrap());
    }
}
#[test]
fn mismatch_still_validates_every_following_cell_and_exact_record_end() {
    let expected = [Value::Integer(0)];
    let mut bytes = encode_row(&[Value::Integer(7), Value::Boolean(true)]).unwrap();
    assert!(!row_matches(&bytes, &expected).unwrap());
    bytes[18] = 2;
    assert_eq!(
        error(&bytes, &expected),
        Error::Decode("boolean").to_string()
    );
    let mut bytes = encode_row(&[Value::Integer(7), Value::Text("x".into())]).unwrap();
    bytes[20] = 255;
    assert_eq!(error(&bytes, &expected), Error::Decode("UTF-8").to_string());
    let mut bytes = encode_row(&[Value::Integer(7), Value::Null]).unwrap();
    bytes.push(0);
    assert_eq!(
        error(&bytes, &expected),
        Error::Decode("trailing bytes").to_string()
    );
}
#[test]
fn truncation_version_tags_float_and_oversized_payload_errors_remain_typed() {
    let row = [Value::Integer(42), Value::Text("я\0".into())];
    let valid = encode_row(&row).unwrap();
    for end in 0..valid.len() {
        let bytes = &valid[..end];
        assert_eq!(
            error(bytes, &row),
            decode_row(bytes).unwrap_err().to_string()
        );
    }
    for (offset, byte) in [(0, 0), (4, 2), (6, 65), (8, 255)] {
        let mut bytes = valid.clone();
        bytes[offset] = byte;
        assert_eq!(
            error(&bytes, &[]),
            decode_row(&bytes).unwrap_err().to_string()
        );
    }
    for float in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut bytes = b"EROW\x01\0\x01\0\x03".to_vec();
        bytes.extend(float.to_bits().to_le_bytes());
        assert_eq!(error(&bytes, &[]), Error::Float.to_string());
    }
    for tag in [4, 5] {
        let mut bytes = b"EROW\x01\0\x01\0".to_vec();
        bytes.push(tag);
        bytes.extend(((MAX_VALUE_BYTES + 1) as u16).to_le_bytes());
        bytes.extend(vec![b'x'; MAX_VALUE_BYTES + 1]);
        assert_eq!(error(&bytes, &[]), Error::ValueSize.to_string());
    }
    assert_eq!(
        error(&vec![0; MAX_ENCODED_BYTES + 1], &[]),
        Error::RecordSize.to_string()
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_payloads_and_mismatch_agree_with_independent_values(id in any::<i64>(),flag in any::<bool>(),text in ".{0,100}",bytes in prop::collection::vec(any::<u8>(),0..256),float in any::<f64>().prop_filter("finite",|v|v.is_finite())) {
        let row=vec![Value::Integer(id),Value::Boolean(flag),Value::Text(text),Value::Bytes(bytes),Value::Float(float),Value::Null];
        let encoded=encode_row(&row).unwrap();prop_assert!(row_matches(&encoded,&row).unwrap());
        let mut changed=row;changed[0]=Value::Integer(id.wrapping_add(1));prop_assert!(!row_matches(&encoded,&changed).unwrap());
    }
    #[test]
    fn arbitrary_records_validate_fully_even_when_expected_count_is_zero(bytes in prop::collection::vec(any::<u8>(),0..5000)) {
        match decode_row(&bytes) {
            Ok(row)=>{prop_assert_eq!(row_matches(&bytes,&[]).unwrap(),row.is_empty());prop_assert!(row_matches(&bytes,&row).unwrap());},
            Err(error)=>prop_assert_eq!(row_matches(&bytes,&[]).unwrap_err().to_string(),error.to_string()),
        }
    }
}
