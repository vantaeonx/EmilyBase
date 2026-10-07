use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{ExecutionError, explain, query};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn wide(count: usize) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "n".into(),
                        data_type: DataType::Integer,
                        nullable: true,
                    },
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "payload".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
                primary_key: 1,
            }),
        })
        .unwrap();
    for id in 0..count {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    if id.is_multiple_of(7) {
                        Value::Null
                    } else {
                        Value::Integer(id as i64 % 5)
                    },
                    Value::Integer(id as i64),
                    Value::Text("x".repeat(3072)),
                ]),
            })
            .unwrap();
    }
    snapshot
}

#[test]
fn primary_order_limit_reads_wide_rows_without_materializing_hidden_fields() {
    let snapshot = wide(6000);
    let digest = snapshot.page_fingerprint();
    let sql = "SELECT x.id AS key FROM t AS x ORDER BY x.id DESC LIMIT 2";
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap().rows,
        vec![vec![Value::Integer(5999)], vec![Value::Integer(5998)]]
    );
    let plan = explain(&snapshot, sql, &[]).unwrap();
    assert_eq!(plan.access, "primary_range");
    assert!(plan.sorted);
    let sql = "SELECT id FROM t WHERE n>=4 ORDER BY id DESC,n ASC NULLS FIRST LIMIT 3";
    let expected = (0..6000i64)
        .rev()
        .filter(|id| id % 7 != 0 && id % 5 >= 4)
        .take(3)
        .map(|id| vec![Value::Integer(id)])
        .collect::<Vec<_>>();
    assert_eq!(query(&snapshot, sql, &[]).unwrap().rows, expected);
    assert_eq!(
        query(&snapshot, "SELECT id FROM t ORDER BY id ASC LIMIT 2", &[])
            .unwrap()
            .rows,
        vec![vec![Value::Integer(0)], vec![Value::Integer(1)]]
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
}

#[test]
fn projected_output_and_unordered_sorting_keep_their_real_memory_bounds() {
    let snapshot = wide(2800);
    for sql in [
        "SELECT payload FROM t ORDER BY id",
        "SELECT * FROM t ORDER BY id DESC",
        "SELECT payload FROM t",
    ] {
        assert!(matches!(
            query(&snapshot, sql, &[]),
            Err(ExecutionError::Limit("output bytes"))
        ));
    }
    // Leading non-primary ordering must still read every candidate before LIMIT.
    assert!(matches!(
        query(&snapshot, "SELECT id FROM t ORDER BY n,id LIMIT 2800", &[]),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    let result = query(
        &snapshot,
        "SELECT payload,payload FROM t ORDER BY id LIMIT 1",
        &[],
    )
    .unwrap();
    assert_eq!(result.rows[0], vec![Value::Text("x".repeat(3072)); 2]);
}

#[test]
fn streamed_star_projection_preserves_every_type_and_float_bits() {
    let mut snapshot = Snapshot::empty().unwrap();
    let fields = [
        ("blob", DataType::Bytes),
        ("flag", DataType::Boolean),
        ("id", DataType::Integer),
        ("f", DataType::Float),
        ("text", DataType::Text),
    ];
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "typed".into(),
                primary_key: 2,
                columns: fields
                    .into_iter()
                    .map(|(name, data_type)| Column {
                        name: name.into(),
                        data_type,
                        nullable: name != "id",
                    })
                    .collect(),
            }),
        })
        .unwrap();
    let rows = [
        vec![
            Value::Bytes(vec![255, 0]),
            Value::Boolean(false),
            Value::Integer(i64::MIN),
            Value::Float(-0.0),
            Value::Text("\0界".into()),
        ],
        vec![
            Value::Null,
            Value::Boolean(true),
            Value::Integer(0),
            Value::Float(1.5),
            Value::Null,
        ],
        vec![
            Value::Bytes(vec![42; 3072]),
            Value::Null,
            Value::Integer(i64::MAX),
            Value::Float(f64::MAX),
            Value::Text("edge".into()),
        ],
    ];
    for index in [1, 2, 0] {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(rows[index].clone()),
            })
            .unwrap();
    }
    let pages = snapshot.page_fingerprint();
    for order in ["ORDER BY id DESC NULLS FIRST,f DESC", "ORDER BY id DESC"] {
        let result = query(&snapshot, &format!("SELECT * FROM typed {order}"), &[]).unwrap();
        assert_eq!(result.columns, ["blob", "flag", "id", "f", "text"]);
        assert_eq!(result.rows, rows.iter().rev().cloned().collect::<Vec<_>>());
        let Value::Float(value) = result.rows[2][3] else {
            panic!("typed float projection")
        };
        assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    }
    assert_eq!(
        query(&snapshot, "SELECT * FROM typed", &[]).unwrap().rows,
        rows
    );
    assert_eq!(snapshot.page_fingerprint(), pages);
}

#[test]
fn empty_and_zero_limit_streams_still_bind_the_entire_statement() {
    for snapshot in [wide(0), wide(20)] {
        for suffix in ["LIMIT 0", "LIMIT 2"] {
            for sql in [
                format!("SELECT absent FROM t ORDER BY id {suffix}"),
                format!("SELECT id FROM t ORDER BY id,absent {suffix}"),
                format!("SELECT id FROM t AS x ORDER BY t.id {suffix}"),
                format!("SELECT id FROM t WHERE id>9 AND id<3 AND absent=1 ORDER BY id {suffix}"),
            ] {
                assert!(matches!(
                    query(&snapshot, &sql, &[]),
                    Err(ExecutionError::Column)
                ));
            }
            let sql = format!("SELECT id FROM t WHERE id>9 AND id<3 AND n=$1 ORDER BY id {suffix}");
            assert!(matches!(
                query(&snapshot, &sql, &[]),
                Err(ExecutionError::Binding(1))
            ));
            assert!(matches!(
                query(&snapshot, &sql, &[Value::Text("wrong".into())]),
                Err(ExecutionError::Type)
            ));
        }
        let result = query(
            &snapshot,
            "SELECT id AS chosen FROM t ORDER BY id LIMIT 0",
            &[],
        )
        .unwrap();
        assert_eq!(result.columns, ["chosen"]);
        assert!(result.rows.is_empty());
        assert!(matches!(
            query(
                &snapshot,
                "SELECT id FROM t ORDER BY id LIMIT $1",
                &[Value::Integer(10001)]
            ),
            Err(ExecutionError::Limit("result rows"))
        ));
    }
}

#[test]
fn primary_order_preserves_int_extremes_point_priority_and_join_fallback() {
    let mut snapshot = wide(10);
    for id in [i64::MIN, i64::MAX] {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(0),
                    Value::Integer(id),
                    Value::Text("edge".into()),
                ]),
            })
            .unwrap();
    }
    for (condition, expected) in [
        ("id>9223372036854775807", vec![]),
        ("id<=-9223372036854775808", vec![i64::MIN]),
        ("id>=9223372036854775807", vec![i64::MAX]),
        ("id<0", vec![i64::MIN]),
        ("id=8 AND n>=3", vec![8]),
        ("id=8 AND n IS NULL", vec![]),
        ("id=8 AND id=9", vec![]),
    ] {
        let sql = format!("SELECT id FROM t WHERE {condition} ORDER BY id DESC LIMIT 1");
        let rows = expected
            .into_iter()
            .map(|id| vec![Value::Integer(id)])
            .collect::<Vec<_>>();
        assert_eq!(query(&snapshot, &sql, &[]).unwrap().rows, rows);
        if condition.starts_with("id=8") {
            assert_eq!(explain(&snapshot, &sql, &[]).unwrap().access, "primary_key");
        }
    }
    let sql = "SELECT a.id,b.id FROM t AS a JOIN t AS b ON a.id=b.id WHERE a.id>=0 AND a.id<10 ORDER BY a.id DESC LIMIT 2";
    assert_eq!(explain(&snapshot, sql, &[]).unwrap().access, "primary_join");
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap().rows,
        vec![vec![Value::Integer(9); 2], vec![Value::Integer(8); 2]]
    );
    let sql = "SELECT id FROM t ORDER BY n DESC NULLS FIRST,id DESC LIMIT 3";
    assert_eq!(explain(&snapshot, sql, &[]).unwrap().access, "scan");
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap().rows,
        vec![
            vec![Value::Integer(7)],
            vec![Value::Integer(0)],
            vec![Value::Integer(9)]
        ]
    );
}

fn model_snapshot(model: &BTreeMap<Key, Option<i64>>, text: bool) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                primary_key: 1,
                columns: vec![
                    Column {
                        name: "n".into(),
                        data_type: DataType::Integer,
                        nullable: true,
                    },
                    Column {
                        name: "id".into(),
                        data_type: if text {
                            DataType::Text
                        } else {
                            DataType::Integer
                        },
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    // Insert in the opposite direction; SQL order must come from the key, not history.
    for (key, n) in model.iter().rev() {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    n.map_or(Value::Null, Value::Integer),
                    key.to_value(),
                ]),
            })
            .unwrap();
    }
    snapshot
}

fn compare_orders(
    model: &BTreeMap<Key, Option<i64>>,
    lower: Key,
    upper: Key,
    limit: usize,
    descending: bool,
) {
    let snapshot = model_snapshot(model, matches!(lower, Key::Text(_)));
    let pages = snapshot.page_fingerprint();
    // TRUE OR UNKNOWN, NOT UNKNOWN and false OR UNKNOWN are independent model cases.
    for (filter, choice) in [
        ("x.id >= $1 AND x.id < $2 AND (x.n IS NULL OR x.n >= 2)", 0),
        ("NOT (x.n < 2) AND NOT (x.id < $1 OR x.id >= $2)", 1),
        ("x.id >= $1 AND x.id < $2 AND (x.n >= 2 OR x.n = NULL)", 2),
        ("(x.id >= $1 AND x.id < $2) OR x.n IS NULL", 3),
    ] {
        let mut expected = model
            .iter()
            .filter(|(key, n)| {
                let in_bounds = **key >= lower && **key < upper;
                match choice {
                    0 => in_bounds && n.is_none_or(|n| n >= 2),
                    1 | 2 => in_bounds && n.is_some_and(|n| n >= 2),
                    _ => in_bounds || n.is_none(),
                }
            })
            .collect::<Vec<_>>();
        if descending {
            expected.reverse();
        }
        let expected = expected
            .into_iter()
            .take(limit)
            .map(|(key, n)| {
                vec![
                    key.to_value(),
                    n.map_or(Value::Null, Value::Integer),
                    key.to_value(),
                ]
            })
            .collect::<Vec<_>>();
        let sql = format!(
            "SELECT x.id AS key,x.n,x.id FROM t AS x WHERE {filter} ORDER BY x.id {} NULLS FIRST,x.n DESC LIMIT $3",
            if descending { "DESC" } else { "ASC" }
        );
        let actual = query(
            &snapshot,
            &sql,
            &[
                lower.to_value(),
                upper.to_value(),
                Value::Integer(limit as i64),
            ],
        )
        .unwrap();
        assert_eq!(actual.columns, ["key", "n", "id"]);
        assert_eq!(actual.rows, expected, "{sql}");
        assert_eq!(
            explain(
                &snapshot,
                &sql,
                &[
                    lower.to_value(),
                    upper.to_value(),
                    Value::Integer(limit as i64)
                ]
            )
            .unwrap()
            .access,
            "primary_range"
        );
    }
    assert_eq!(snapshot.page_fingerprint(), pages);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_integer_primary_orders_filter_before_limit(
        input in prop::collection::btree_map(-200i64..200,prop::option::of(-5i64..7),0..160),
        lower in -240i64..240,upper in -240i64..240,limit in 0usize..165,descending in any::<bool>()
    ) {
        let model = input.into_iter().map(|(id,n)|(Key::Integer(id),n)).collect();
        compare_orders(&model,Key::Integer(lower),Key::Integer(upper),limit,descending);
    }
    #[test]
    fn generated_text_primary_orders_merge_long_keys_with_null_and_unicode(
        input in prop::collection::btree_map("[a-zé界\0]{0,12}",prop::option::of(-5i64..7),0..80),
        lower in 0usize..11,upper in 0usize..11,limit in 0usize..90,descending in any::<bool>()
    ) {
        let keys = [String::new(),"\0".into(),"a".into(),"a\0".into(),"b".into(),"界".into(),"😀".into(),"z".repeat(255),"z".repeat(256),"z".repeat(257),format!("a{}","x".repeat(3071))];
        let mut model = input.into_iter().map(|(id,n)|(Key::Text(id),n)).collect::<BTreeMap<_,_>>();
        for (index,key) in keys.iter().enumerate() {model.insert(Key::Text(key.clone()),if index%3==0 {None}else{Some(index as i64%5)});}
        compare_orders(&model,Key::Text(keys[lower].clone()),Key::Text(keys[upper].clone()),limit,descending);
    }
}
