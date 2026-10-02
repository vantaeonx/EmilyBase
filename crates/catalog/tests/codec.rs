use emilybase_catalog::{
    Column, DataType, Error, MAX_COLUMNS, MAX_ENCODED_BYTES, Schema, Value, decode_row,
    decode_schema, encode_row, encode_schema,
};
use proptest::prelude::*;

fn schema() -> Schema {
    Schema {
        name: "t".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    }
}

#[test]
fn stable_synthetic_golden_records() {
    let bytes = encode_schema(&schema()).unwrap();
    assert_eq!(bytes, b"ESCH\x01\0\0\0\x01\0\x01\0t\x02\0id\x02\0");
    assert_eq!(decode_schema(&bytes).unwrap(), schema());
    let row = vec![Value::Integer(42)];
    let bytes = encode_row(&row).unwrap();
    assert_eq!(bytes, b"EROW\x01\0\x01\0\x02\x2a\0\0\0\0\0\0\0");
    assert_eq!(decode_row(&bytes).unwrap(), row);
}

#[test]
fn every_value_kind_round_trips() {
    let row = vec![
        Value::Null,
        Value::Boolean(false),
        Value::Boolean(true),
        Value::Integer(i64::MIN),
        Value::Integer(i64::MAX),
        Value::Float(-0.0),
        Value::Float(f64::MAX),
        Value::Text("Привет".into()),
        Value::Bytes(vec![0, 255]),
    ];
    assert_eq!(decode_row(&encode_row(&row).unwrap()).unwrap(), row);
}

#[test]
fn every_truncated_prefix_and_trailing_data_are_rejected() {
    let bytes = encode_schema(&schema()).unwrap();
    for end in 0..bytes.len() {
        assert!(decode_schema(&bytes[..end]).is_err());
    }
    let bytes = encode_row(&[Value::Text("payload".into())]).unwrap();
    for end in 0..bytes.len() {
        assert!(decode_row(&bytes[..end]).is_err());
    }
    let mut bytes = bytes;
    bytes.push(0);
    assert!(decode_row(&bytes).is_err());
}

#[test]
fn unknown_versions_tags_and_invalid_utf8_fail_closed() {
    let mut bytes = encode_row(&[Value::Text("x".into())]).unwrap();
    bytes[4] = 2;
    assert!(matches!(decode_row(&bytes), Err(Error::Version(2))));
    bytes[4] = 1;
    bytes[8] = 255;
    assert!(decode_row(&bytes).is_err());
    bytes[8] = 4;
    bytes[11] = 255;
    assert!(decode_row(&bytes).is_err());
    let mut bytes = encode_row(&[Value::Boolean(true)]).unwrap();
    bytes[9] = 2;
    assert!(decode_row(&bytes).is_err());
}

#[test]
fn record_size_is_bounded_before_allocation() {
    assert!(decode_row(&vec![0; MAX_ENCODED_BYTES + 1]).is_err());
    assert!(decode_schema(&vec![0; MAX_ENCODED_BYTES + 1]).is_err());
    assert!(matches!(
        encode_row(&vec![Value::Null; MAX_COLUMNS + 1]),
        Err(Error::RowLength)
    ));
    assert!(matches!(
        encode_row(&[Value::Text("x".repeat(3000)), Value::Bytes(vec![1; 3000])]),
        Err(Error::RecordSize)
    ));
}

#[test]
fn malformed_schema_fields_cannot_bypass_validation() {
    let valid = encode_schema(&schema()).unwrap();
    for (offset, replacement) in [(6, 255), (8, 255), (12, 0), (17, 255), (18, 2)] {
        let mut bytes = valid.clone();
        bytes[offset] = replacement;
        assert!(decode_schema(&bytes).is_err());
    }
    let mut bytes = valid;
    bytes[13] = 255;
    bytes[14] = 255;
    assert!(decode_schema(&bytes).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn integer_and_binary_rows_round_trip(
        key in any::<i64>(),
        payload in prop::collection::vec(any::<u8>(), 0..3000),
        flag in any::<bool>(),
    ) {
        let row = vec![Value::Integer(key), Value::Bytes(payload), Value::Boolean(flag)];
        prop_assert_eq!(decode_row(&encode_row(&row).unwrap()).unwrap(), row);
    }

    #[test]
    fn arbitrary_catalog_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..5000)) {
        let _ = decode_schema(&bytes);
        let _ = decode_row(&bytes);
    }

    #[test]
    fn mutated_valid_records_are_rejected_or_canonical(
        offset in 0usize..20,
        replacement in any::<u8>(),
    ) {
        let mut bytes = encode_schema(&schema()).unwrap();
        let index = offset % bytes.len();
        bytes[index] = replacement;
        if let Ok(schema) = decode_schema(&bytes) {
            prop_assert_eq!(decode_schema(&encode_schema(&schema).unwrap()).unwrap(), schema);
        }
    }

    #[test]
    fn finite_float_bits_and_unicode_text_survive(
        value in any::<f64>().prop_filter("finite", |value| value.is_finite()),
        text in ".{0,100}",
    ) {
        let row = vec![Value::Float(value), Value::Text(text.clone())];
        let decoded = decode_row(&encode_row(&row).unwrap()).unwrap();
        if let Value::Float(actual) = decoded[0] {
            prop_assert_eq!(actual.to_bits(), value.to_bits());
        } else {
            prop_assert!(false, "float tag was not preserved");
        }
        prop_assert_eq!(&decoded[1], &Value::Text(text));
    }
}
