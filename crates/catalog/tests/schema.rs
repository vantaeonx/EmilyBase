use emilybase_catalog::{
    Column, DataType, Error, Key, MAX_COLUMNS, MAX_NAME_BYTES, MAX_VALUE_BYTES, Schema, Value,
};

fn schema() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "title".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
        primary_key: 0,
    }
}

#[test]
fn valid_rows_and_keys_are_typed() {
    let schema = schema();
    schema.validate().unwrap();
    let row = vec![Value::Integer(7), Value::Text("synthetic".into())];
    assert_eq!(schema.key(&row).unwrap(), Key::Integer(7));
    schema
        .validate_row(&[Value::Integer(-7), Value::Null])
        .unwrap();
    schema.validate_key(&Key::Integer(i64::MAX)).unwrap();
    assert!(schema.validate_key(&Key::Text("7".into())).is_err());
}

#[test]
fn invalid_identifiers_are_rejected() {
    for name in ["", "7items", "a/b", "a.b", "a b", "a-b", "таблица", "a\0b"] {
        let mut candidate = schema();
        candidate.name = name.into();
        assert!(matches!(candidate.validate(), Err(Error::Identifier)));
    }
    let mut candidate = schema();
    candidate.name = "a".repeat(MAX_NAME_BYTES + 1);
    assert!(candidate.validate().is_err());
    candidate.name = "_".repeat(MAX_NAME_BYTES);
    candidate.validate().unwrap();
}

#[test]
fn primary_key_constraints_and_duplicate_columns_are_enforced() {
    let mut candidate = schema();
    candidate.primary_key = 10;
    assert!(matches!(candidate.validate(), Err(Error::PrimaryKey)));
    candidate.primary_key = 0;
    candidate.columns[0].nullable = true;
    assert!(matches!(candidate.validate(), Err(Error::PrimaryKey)));
    candidate.columns[0].nullable = false;
    candidate.columns[0].data_type = DataType::Float;
    assert!(matches!(candidate.validate(), Err(Error::PrimaryKey)));
    candidate.columns[0].data_type = DataType::Text;
    candidate.columns[1].name = "id".into();
    assert!(matches!(candidate.validate(), Err(Error::DuplicateColumn)));
}

#[test]
fn row_shape_and_nullability_are_checked() {
    let schema = schema();
    assert!(matches!(schema.validate_row(&[]), Err(Error::RowLength)));
    assert!(matches!(
        schema.validate_row(&[Value::Null, Value::Null]),
        Err(Error::Null(0))
    ));
    assert!(matches!(
        schema.validate_row(&[Value::Text("7".into()), Value::Null]),
        Err(Error::Type(0))
    ));
    assert!(
        schema
            .validate_row(&[Value::Integer(7), Value::Boolean(true)])
            .is_err()
    );
}

#[test]
fn unsupported_sizes_and_nonfinite_numbers_are_rejected() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(Value::Float(value).validate(), Err(Error::Float)));
    }
    assert!(Value::Float(f64::MIN).validate().is_ok());
    assert!(Value::Text("x".repeat(MAX_VALUE_BYTES)).validate().is_ok());
    assert!(matches!(
        Value::Bytes(vec![0; MAX_VALUE_BYTES + 1]).validate(),
        Err(Error::ValueSize)
    ));
    let mut candidate = schema();
    candidate.columns.clear();
    assert!(matches!(candidate.validate(), Err(Error::ColumnCount)));
    candidate.columns = (0..=MAX_COLUMNS)
        .map(|index| Column {
            name: format!("c{index}"),
            data_type: DataType::Integer,
            nullable: false,
        })
        .collect();
    assert!(candidate.validate().is_err());
}

#[test]
fn json_values_are_explicit_and_unknown_fields_are_rejected() {
    let value: Value = serde_json::from_str(r#"{"type":"integer","value":7}"#).unwrap();
    assert_eq!(value, Value::Integer(7));
    assert!(serde_json::from_str::<Value>(r#"{"type":"integer","value":7,"extra":1}"#).is_err());
    assert!(serde_json::from_str::<Value>(r#"{"type":"null","value":5}"#).is_err());
    assert!(serde_json::from_str::<Key>(r#"{"type":"float","value":1.0}"#).is_err());
}
