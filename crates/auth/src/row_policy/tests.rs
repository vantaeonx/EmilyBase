use super::*;
use emilybase_catalog::{Column, DataType};
use proptest::prelude::*;
pub(crate) const PROJECT: &str = "11111111111111111111111111111111";
pub(crate) fn schema() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "owner".into(),
                data_type: DataType::Bytes,
                nullable: true,
            },
            Column {
                name: "visible".into(),
                data_type: DataType::Boolean,
                nullable: false,
            },
            Column {
                name: "note".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
        primary_key: 0,
    }
}
pub(crate) fn definition(select: Rule) -> Definition {
    Definition {
        version: 1,
        select,
        insert: Rule::Deny {},
        update_using: Rule::Deny {},
        update_check: Rule::Deny {},
        delete: Rule::Deny {},
    }
}
fn bind(schema: &Schema, d: &Definition) -> Result<BoundPolicy> {
    BoundPolicy::compile(
        TableContext {
            project: PROJECT,
            id: 1,
            schema,
        },
        d,
    )
}
fn json() -> serde_json::Value {
    serde_json::json!({"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}})
}
#[test]
fn policy_json_is_bounded_explicit_and_rejects_unknown_duplicate_and_malformed_fields() {
    let mut bad = vec![
        vec![0xff],
        vec![b' '; MAX_DOCUMENT_BYTES + 1],
        b"{}".to_vec(),
        b"[]".to_vec(),
    ];
    let mut input = json();
    input.as_object_mut().unwrap().remove("delete");
    bad.push(input.to_string().into_bytes());
    let mut input = json();
    input["path"] = serde_json::json!("elsewhere");
    bad.push(input.to_string().into_bytes());
    let mut input = json();
    input["select"] = serde_json::json!({"kind":"deny","extra":true});
    bad.push(input.to_string().into_bytes());
    let mut input = json();
    input["select"] = serde_json::json!({"kind":"authenticated","extra":true});
    bad.push(input.to_string().into_bytes());
    let mut input = json();
    input["select"] = serde_json::json!({"kind":"unknown"});
    bad.push(input.to_string().into_bytes());
    let mut input = json();
    input["select"] = serde_json::json!({"kind":"owner"});
    bad.push(input.to_string().into_bytes());
    let mut duplicate = json().to_string();
    duplicate.insert_str(1, "\"version\":1,");
    bad.push(duplicate.into_bytes());
    for bytes in bad {
        assert!(
            decode(&bytes).is_err(),
            "strict decoder accepted an invalid document"
        );
    }
    assert!(bind(&schema(), &decode(json().to_string().as_bytes()).unwrap()).is_ok());
}
#[test]
fn compilation_rejects_invalid_context_version_column_type_null_and_nonfinite_literals() {
    let s = schema();
    let d = definition(Rule::Deny {});
    for (project, id) in [("../outside", 1), (PROJECT, 0)] {
        assert!(matches!(
            BoundPolicy::compile(
                TableContext {
                    project,
                    id,
                    schema: &s
                },
                &d
            ),
            Err(PolicyError::Scope)
        ));
    }
    let mut d = definition(Rule::Deny {});
    d.version = 2;
    assert!(matches!(bind(&s, &d), Err(PolicyError::Version)));
    for rule in [
        Rule::Owner {
            column: "id".into(),
        },
        Rule::Owner {
            column: "missing".into(),
        },
        Rule::IsNull {
            column: "id".into(),
        },
        Rule::Equal {
            column: "note".into(),
            value: Value::Null,
        },
        Rule::Equal {
            column: "id".into(),
            value: Value::Text("secret".into()),
        },
        Rule::Equal {
            column: "note".into(),
            value: Value::Text("x".repeat(3073)),
        },
    ] {
        assert!(matches!(
            bind(&s, &definition(rule)),
            Err(PolicyError::Schema)
        ));
    }
    let mut s = s;
    s.columns[3].data_type = DataType::Float;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            bind(
                &s,
                &definition(Rule::Equal {
                    column: "note".into(),
                    value: Value::Float(value)
                })
            )
            .is_err()
        );
    }
    let mut s = schema();
    s.columns[0].nullable = true;
    assert!(matches!(
        bind(&s, &definition(Rule::Deny {})),
        Err(PolicyError::Schema)
    ));
}
#[test]
fn aggregate_node_depth_and_literal_budgets_refuse_before_unbounded_owned_compilation() {
    let s = schema();
    let all = |n| Rule::All {
        terms: (0..n).map(|_| Rule::Deny {}).collect(),
    };
    assert!(bind(&s, &definition(all(59))).is_ok()); // 60 + four remaining roots.
    assert!(matches!(
        bind(&s, &definition(all(60))),
        Err(PolicyError::Limit)
    ));
    for rule in [
        Rule::All { terms: vec![] },
        Rule::Any { terms: vec![] },
        all(100),
    ] {
        assert!(matches!(
            bind(&s, &definition(rule)),
            Err(PolicyError::Limit)
        ));
    }
    let nested = |depth| {
        let mut r = Rule::Deny {};
        for _ in 1..depth {
            r = Rule::All { terms: vec![r] };
        }
        r
    };
    assert!(bind(&s, &definition(nested(MAX_DEPTH))).is_ok());
    assert!(matches!(
        bind(&s, &definition(nested(MAX_DEPTH + 1))),
        Err(PolicyError::Limit)
    ));
    let literals = |last| Rule::All {
        terms: [3072, 3072, last]
            .into_iter()
            .map(|n| Rule::Equal {
                column: "note".into(),
                value: Value::Text("x".repeat(n)),
            })
            .collect(),
    };
    assert!(bind(&s, &definition(literals(2048))).is_ok());
    assert!(matches!(
        bind(&s, &definition(literals(2049))),
        Err(PolicyError::Limit)
    ));
}
#[test]
fn policy_debug_and_errors_never_print_literals_or_scoped_identity() {
    let s = schema();
    let d = definition(Rule::Equal {
        column: "note".into(),
        value: Value::Text("synthetic-sensitive-policy".into()),
    });
    let bound = bind(&s, &d).unwrap();
    for text in [
        format!("{d:?}"),
        format!("{:?}", d.select),
        format!("{bound:?}"),
        PolicyError::Denied.to_string(),
        PolicyError::Schema.to_string(),
    ] {
        assert!(!text.contains("synthetic-sensitive-policy"));
        assert!(!text.contains(PROJECT));
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_bounded_documents_can_only_compile_valid_explicit_models(bytes in prop::collection::vec(any::<u8>(),0..17000)) {
        if let Ok(definition)=decode(&bytes) {let _=bind(&schema(),&definition);}
    }
}
