use super::*;
use proptest::prelude::*;
use std::collections::BTreeSet;
fn make(names: &[&str]) -> Schema {
    Schema {
        name: "t".into(),
        columns: names
            .iter()
            .map(|name| Column {
                name: (*name).into(),
                data_type: DataType::Integer,
                nullable: false,
            })
            .collect(),
        primary_key: 0,
    }
}
fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 63
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn model(schema: &Schema) -> Option<&'static str> {
    if !identifier(&schema.name) {
        return Some("identifier");
    }
    if schema.columns.is_empty() || schema.columns.len() > 64 {
        return Some("count");
    }
    let mut seen = BTreeSet::new();
    for c in &schema.columns {
        if !identifier(&c.name) {
            return Some("identifier");
        }
        if !seen.insert(&c.name) {
            return Some("duplicate");
        }
    }
    let Some(primary) = schema.columns.get(usize::from(schema.primary_key)) else {
        return Some("primary");
    };
    if primary.nullable || !matches!(primary.data_type, DataType::Integer | DataType::Text) {
        return Some("primary");
    }
    None
}
fn kind(result: Result<()>) -> Option<&'static str> {
    match result {
        Ok(()) => None,
        Err(Error::Identifier) => Some("identifier"),
        Err(Error::ColumnCount) => Some("count"),
        Err(Error::DuplicateColumn) => Some("duplicate"),
        Err(Error::PrimaryKey) => Some("primary"),
        Err(_) => panic!("unexpected schema error"),
    }
}
#[test]
fn duplicate_identity_and_invalid_identifier_keep_original_first_error() {
    for (names, error) in [
        (vec!["x", "x", "-"], Some("duplicate")),
        (vec!["x", "-", "x"], Some("identifier")),
        (vec!["-", "-"], Some("identifier")),
        (vec!["x", "y", "x", "y"], Some("duplicate")),
        (vec!["a", "A", "_a"], None),
    ] {
        assert_eq!(kind(make(&names).validate()), error);
    }
    let mut schema = make(&["x", "x"]);
    schema.primary_key = u16::MAX;
    assert_eq!(kind(schema.validate()), Some("duplicate"));
    schema.name = "-".into();
    schema.columns.clear();
    assert_eq!(kind(schema.validate()), Some("identifier"));
}
#[test]
fn every_duplicate_position_and_maximum_name_count_is_exact() {
    let schema = Schema {
        name: "_".repeat(MAX_NAME_BYTES),
        columns: (0..MAX_COLUMNS)
            .map(|i| Column {
                name: format!("c{i:02}_{}", "x".repeat(MAX_NAME_BYTES - 4)),
                data_type: DataType::Integer,
                nullable: false,
            })
            .collect(),
        primary_key: (MAX_COLUMNS - 1) as u16,
    };
    schema.validate().unwrap();
    for first in 0..MAX_COLUMNS {
        for second in first + 1..MAX_COLUMNS {
            let mut changed = schema.clone();
            changed.columns[second].name = changed.columns[first].name.clone();
            assert_eq!(kind(changed.validate()), Some("duplicate"));
        }
    }
    let mut oversized = schema;
    oversized.columns.push(oversized.columns[0].clone());
    assert_eq!(kind(oversized.validate()), Some("count"));
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn bounded_stack_validation_matches_independent_sequential_model(
        table in prop::sample::select(vec!["t","_valid","","9bad","я","a\0b"]),
        fields in prop::collection::vec((0u8..40,any::<bool>(),0u8..5),0..70),primary in 0u16..70,
    ) {
        let columns=fields.into_iter().map(|(name,nullable,kind)|Column{name:match name%10{0=>"".into(),1=>"-".into(),2=>"я".into(),_=>format!("c{name}")},data_type:match kind{0=>DataType::Boolean,1=>DataType::Integer,2=>DataType::Float,3=>DataType::Text,_=>DataType::Bytes},nullable}).collect();
        let schema=Schema{name:table.into(),columns,primary_key:primary};
        prop_assert_eq!(kind(schema.validate()),model(&schema));
    }
}

#[test]
fn oversized_later_names_preserve_earlier_duplicate_refusal() {
    let mut schema = make(&["x", "x", "y"]);
    schema.columns[2].name = "x".repeat(1024 * 1024);
    assert_eq!(kind(schema.validate()), Some("duplicate"));
    schema.columns[1].name = schema.columns[2].name.clone();
    assert_eq!(kind(schema.validate()), Some("identifier"));
}
