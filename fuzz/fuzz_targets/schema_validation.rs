#![no_main]
#![forbid(unsafe_code)]
use emilybase_catalog::{Column, DataType, Error, Schema};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;
fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 63
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn model(schema: &Schema) -> Option<u8> {
    if !identifier(&schema.name) {
        return Some(1);
    }
    if schema.columns.is_empty() || schema.columns.len() > 64 {
        return Some(2);
    }
    let mut seen = BTreeSet::new();
    for c in &schema.columns {
        if !identifier(&c.name) {
            return Some(1);
        }
        if !seen.insert(&c.name) {
            return Some(3);
        }
    }
    let Some(primary) = schema.columns.get(usize::from(schema.primary_key)) else {
        return Some(4);
    };
    if primary.nullable || !matches!(primary.data_type, DataType::Integer | DataType::Text) {
        return Some(4);
    }
    None
}
fuzz_target!(|bytes: &[u8]| {
    if bytes.len() < 3 || bytes.len() > 512 {
        return;
    }
    let name = match bytes[0] % 6 {
        0 => "t",
        1 => "_",
        2 => "",
        3 => "7bad",
        4 => "я",
        _ => "a\0b",
    };
    let primary_key = u16::from_le_bytes([bytes[1], bytes[2]]);
    let columns = bytes[3..]
        .as_chunks::<3>()
        .0
        .iter()
        .take(80)
        .map(|cmd| Column {
            name: match cmd[0] % 8 {
                0 => "".into(),
                1 => "-".into(),
                2 => "я".into(),
                3 => "_".repeat(63),
                4 => "_".repeat(64),
                _ => format!("c{}", cmd[0] % 32),
            },
            data_type: match cmd[1] % 5 {
                0 => DataType::Integer,
                1 => DataType::Text,
                2 => DataType::Boolean,
                3 => DataType::Float,
                _ => DataType::Bytes,
            },
            nullable: cmd[2] & 1 != 0,
        })
        .collect();
    let schema = Schema {
        name: name.into(),
        columns,
        primary_key,
    };
    let original = schema.clone();
    let observed = match schema.validate() {
        Ok(()) => None,
        Err(Error::Identifier) => Some(1),
        Err(Error::ColumnCount) => Some(2),
        Err(Error::DuplicateColumn) => Some(3),
        Err(Error::PrimaryKey) => Some(4),
        Err(error) => panic!("unexpected schema error: {error}"),
    };
    assert_eq!(observed, model(&schema));
    assert_eq!(schema, original);
});
