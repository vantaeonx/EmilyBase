use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

fn fixture(count: usize) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "left_rows"), (2, "right_rows")] {
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: vec![
                        Column {
                            name: "id".into(),
                            data_type: DataType::Integer,
                            nullable: false,
                        },
                        Column {
                            name: "link".into(),
                            data_type: DataType::Integer,
                            nullable: true,
                        },
                    ],
                    primary_key: 0,
                }),
            })
            .unwrap();
        for number in 0..count {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(number as i64),
                        Value::Integer(number as i64),
                    ]),
                })
                .unwrap();
        }
    }
    snapshot
}
#[test]
fn primary_join_does_not_exhaust_work_on_irrelevant_right_rows() {
    let snapshot = fixture(400);
    let old = snapshot.clone();
    let fingerprint = old.page_fingerprint();
    let result = query(&snapshot,"SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 3",&[]).unwrap();
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Integer(399), Value::Integer(399)],
            vec![Value::Integer(398), Value::Integer(398)],
            vec![Value::Integer(397), Value::Integer(397)]
        ]
    );
    assert_eq!(snapshot.page_fingerprint(), fingerprint);
    assert_eq!(old.page_fingerprint(), fingerprint);
}

use emilybase_catalog::{Key, Row};
use emilybase_query::{ExecutionError, explain};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn add_table(
    snapshot: &mut Snapshot,
    id: u64,
    name: &str,
    kinds: &[(&str, DataType, bool)],
    primary: u16,
) {
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                columns: kinds
                    .iter()
                    .map(|(name, kind, nullable)| Column {
                        name: (*name).into(),
                        data_type: *kind,
                        nullable: *nullable,
                    })
                    .collect(),
                primary_key: primary,
            }),
        })
        .unwrap();
}
fn insert(snapshot: &mut Snapshot, id: u64, row: Row) {
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Insert(row),
        })
        .unwrap();
}
fn parity(snapshot: &Snapshot, on: &str, suffix: &str, parameters: &[Value]) {
    let fast =
        format!("SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON {on} {suffix}");
    let reference = format!(
        "SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON ({on}) OR FALSE {suffix}"
    );
    assert_eq!(
        explain(snapshot, &fast, parameters).unwrap().access,
        "primary_join"
    );
    assert_eq!(
        explain(snapshot, &reference, parameters).unwrap().access,
        "bounded_nested_loop"
    );
    assert_eq!(
        query(snapshot, &fast, parameters).unwrap(),
        query(snapshot, &reference, parameters).unwrap()
    );
}

#[test]
fn reversed_necessary_equalities_preserve_full_on_where_sort_and_limit() {
    let snapshot = fixture(30);
    for on in [
        "a.link=b.id",
        "b.id=a.link",
        "a.link=b.id AND b.link>=10",
        "(b.link>=10 AND b.id=a.link) AND a.id<25",
        "a.link=b.id AND a.id=b.link",
    ] {
        for suffix in [
            "",
            "LIMIT 0",
            "LIMIT 2",
            "ORDER BY b.link DESC,a.id LIMIT 7",
            "WHERE a.id>=$1 ORDER BY a.id DESC LIMIT $2",
        ] {
            parity(
                &snapshot,
                on,
                suffix,
                &[Value::Integer(5), Value::Integer(3)],
            );
        }
    }
    let sql = "SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id AND a.id=b.id AND b.id=a.link";
    assert_eq!(query(&snapshot, sql, &[]).unwrap().rows.len(), 30);
}

#[test]
fn null_missing_and_repeated_foreign_keys_keep_left_scan_order() {
    let mut snapshot = fixture(0);
    let links = [
        Value::Integer(2),
        Value::Null,
        Value::Integer(99),
        Value::Integer(2),
        Value::Integer(1),
    ];
    for (number, link) in links.into_iter().enumerate() {
        insert(&mut snapshot, 1, vec![Value::Integer(number as i64), link]);
    }
    for number in [1, 2] {
        insert(
            &mut snapshot,
            2,
            vec![Value::Integer(number), Value::Integer(number)],
        );
    }
    let sql = "SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id";
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap().rows,
        vec![
            vec![Value::Integer(0), Value::Integer(2)],
            vec![Value::Integer(3), Value::Integer(2)],
            vec![Value::Integer(4), Value::Integer(1)]
        ]
    );
    parity(
        &snapshot,
        "a.link=b.id",
        "ORDER BY b.id DESC,a.id DESC LIMIT 2",
        &[],
    );
    parity(
        &snapshot,
        "a.link=b.id AND a.link IS NOT NULL",
        "WHERE b.id=2 LIMIT 1",
        &[],
    );
}

#[test]
fn primary_columns_in_nonzero_positions_and_self_aliases_are_resolved() {
    let mut snapshot = Snapshot::empty().unwrap();
    add_table(
        &mut snapshot,
        1,
        "t",
        &[
            ("link", DataType::Integer, true),
            ("id", DataType::Integer, false),
        ],
        1,
    );
    for (id, link) in [(1, 2), (2, 1), (3, 2)] {
        insert(
            &mut snapshot,
            1,
            vec![Value::Integer(link), Value::Integer(id)],
        );
    }
    for on in ["a.link=b.id", "b.id=a.link"] {
        let sql = format!(
            "SELECT a.id AS left_id,b.id AS right_id FROM t AS a JOIN t AS b ON {on} ORDER BY a.id"
        );
        let plan = explain(&snapshot, &sql, &[]).unwrap();
        assert_eq!(plan.access, "primary_join");
        assert_eq!(plan.joined_table.as_deref(), Some("t"));
        assert!(plan.sorted);
        let result = query(&snapshot, &sql, &[]).unwrap();
        assert_eq!(result.columns, ["left_id", "right_id"]);
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Integer(1), Value::Integer(2)],
                vec![Value::Integer(2), Value::Integer(1)],
                vec![Value::Integer(3), Value::Integer(2)]
            ]
        );
    }
    let star = query(
        &snapshot,
        "SELECT * FROM t AS a JOIN t AS b ON a.link=b.id LIMIT 1",
        &[],
    )
    .unwrap();
    assert_eq!(star.columns, ["a.link", "a.id", "b.link", "b.id"]);
    assert_eq!(
        star.rows,
        [vec![
            Value::Integer(2),
            Value::Integer(1),
            Value::Integer(1),
            Value::Integer(2)
        ]]
    );
}

#[test]
fn utf8_nul_and_long_excluded_keys_use_the_same_validated_point_path() {
    let mut snapshot = Snapshot::empty().unwrap();
    add_table(
        &mut snapshot,
        1,
        "left_rows",
        &[
            ("id", DataType::Integer, false),
            ("link", DataType::Text, true),
        ],
        0,
    );
    add_table(
        &mut snapshot,
        2,
        "right_rows",
        &[
            ("n", DataType::Integer, false),
            ("id", DataType::Text, false),
        ],
        1,
    );
    let texts = [
        String::new(),
        "я\0".into(),
        "é".into(),
        "é".into(),
        "x".repeat(255),
        "x".repeat(256),
        "x".repeat(257),
        "🌌".repeat(768),
    ];
    let mut expected = Vec::new();
    for (number, text) in texts.iter().enumerate() {
        insert(
            &mut snapshot,
            1,
            vec![Value::Integer(number as i64), Value::Text(text.clone())],
        );
        insert(
            &mut snapshot,
            2,
            vec![Value::Integer(number as i64), Value::Text(text.clone())],
        );
        expected.push(vec![
            Value::Integer(number as i64),
            Value::Text(text.clone()),
        ]);
    }
    insert(&mut snapshot, 1, vec![Value::Integer(90), Value::Null]);
    insert(
        &mut snapshot,
        1,
        vec![Value::Integer(91), Value::Text("missing".repeat(400))],
    );
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let sql =
        "SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id ORDER BY a.id";
    assert_eq!(query(&snapshot, sql, &[]).unwrap().rows, expected);
    assert_eq!(explain(&snapshot, sql, &[]).unwrap().access, "primary_join");
    for (number, text) in texts.iter().enumerate() {
        snapshot
            .apply(Event {
                table_id: 2,
                kind: EventKind::Replace(vec![
                    Value::Integer(100 + number as i64),
                    Value::Text(text.clone()),
                ]),
            })
            .unwrap();
    }
    assert_eq!(query(&old, sql, &[]).unwrap().rows, expected);
    assert_eq!(old.page_fingerprint(), digest);
    assert_eq!(query(&snapshot, sql, &[]).unwrap().rows, expected);
    snapshot
        .apply(Event {
            table_id: 2,
            kind: EventKind::Delete(Key::Text(texts[7].clone())),
        })
        .unwrap();
    assert_eq!(query(&snapshot, sql, &[]).unwrap().rows, expected[..7]);
    assert_eq!(query(&old, sql, &[]).unwrap().rows, expected);
}

#[test]
fn long_left_primary_keys_merge_into_original_unsorted_join_order() {
    let mut snapshot = Snapshot::empty().unwrap();
    add_table(&mut snapshot, 1, "t", &[("id", DataType::Text, false)], 0);
    let mut keys = [
        "a".into(),
        format!("a{}", "z".repeat(3071)),
        "b".into(),
        "a\0".into(),
        "я".into(),
    ];
    for key in &keys {
        insert(&mut snapshot, 1, vec![Value::Text(key.clone())]);
    }
    keys.sort();
    let sql = "SELECT a.id FROM t AS a JOIN t AS b ON a.id=b.id LIMIT 4";
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap().rows,
        keys[..4]
            .iter()
            .map(|v| vec![Value::Text(v.clone())])
            .collect::<Vec<_>>()
    );
}

#[test]
fn or_not_nonprimary_same_side_and_literal_conditions_keep_nested_plan() {
    let snapshot = fixture(5);
    for on in [
        "a.link=b.link",
        "a.id=b.id OR TRUE",
        "NOT(a.id<>b.id)",
        "b.id=b.link",
        "a.id=a.link",
        "b.id=1",
        "a.id=b.id OR a.id=4",
    ] {
        let sql = format!(
            "SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON {on} ORDER BY a.id,b.id"
        );
        assert_eq!(
            explain(&snapshot, &sql, &[]).unwrap().access,
            "bounded_nested_loop",
            "{on}"
        );
        query(&snapshot, &sql, &[]).unwrap();
    }
    let sql = "SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON a.id=b.id AND (a.link<3 OR b.link>4)";
    assert_eq!(explain(&snapshot, sql, &[]).unwrap().access, "primary_join");
    assert_eq!(query(&snapshot, sql, &[]).unwrap().rows.len(), 3);
}

#[test]
fn complete_schema_and_parameter_validation_precedes_empty_or_zero_limit() {
    let snapshot = fixture(0);
    for sql in [
        "SELECT id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id LIMIT 0",
        "SELECT a.id FROM left_rows AS a JOIN right_rows AS b ON a.missing=b.id LIMIT 0",
        "SELECT a.id FROM left_rows AS a JOIN right_rows AS a ON a.link=a.id LIMIT 0",
    ] {
        assert!(matches!(
            query(&snapshot, sql, &[]),
            Err(ExecutionError::Column)
        ));
    }
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id AND b.link='wrong' LIMIT 0",
            &[]
        ),
        Err(ExecutionError::Type)
    ));
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id WHERE a.id=$1 LIMIT 0",
            &[]
        ),
        Err(ExecutionError::Binding(1))
    ));
    assert_eq!(
        query(
            &snapshot,
            "SELECT a.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id LIMIT 0",
            &[]
        )
        .unwrap()
        .columns,
        ["id"]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_nullable_many_to_one_joins_match_an_independent_map(
        incoming in prop::collection::vec(prop::option::of(-12i64..24),0..48),
        right in prop::collection::btree_map(-12i64..24,-50i64..50,0..24),
        descending in any::<bool>(), limit in 0usize..32,
    ) {
        let mut snapshot=fixture(0);
        let mut left=BTreeMap::new();
        for (number,link) in incoming.into_iter().enumerate() {
            insert(&mut snapshot,1,vec![Value::Integer(number as i64),link.map_or(Value::Null,Value::Integer)]);
            left.insert(number as i64,link);
        }
        for (key,value) in &right {insert(&mut snapshot,2,vec![Value::Integer(*key),Value::Integer(*value)]);}
        let old=snapshot.clone();
        let digest=old.page_fingerprint();
        let mut expected:Vec<_>=left.iter().filter_map(|(id,link)| {
            let key=(*link)?;
            let payload=*right.get(&key)?;
            (payload>=0).then_some(vec![Value::Integer(*id),Value::Integer(key)])
        }).collect();
        if descending {expected.reverse();}
        expected.truncate(limit);
        let order=if descending {"DESC"} else {"ASC"};
        let sql=format!("SELECT a.id,b.id FROM left_rows AS a JOIN right_rows AS b ON b.id=a.link AND b.link>=$1 ORDER BY a.id {order} LIMIT $2");
        let values=[Value::Integer(0),Value::Integer(limit as i64)];
        prop_assert_eq!(query(&snapshot,&sql,&values).unwrap().rows,expected);
        prop_assert_eq!(explain(&snapshot,&sql,&values).unwrap().access,"primary_join");
        parity(&snapshot,"b.id=a.link AND b.link>=$1",&format!("ORDER BY a.id {order} LIMIT $2"),&values);
        prop_assert_eq!(snapshot.page_fingerprint(),digest);
        prop_assert_eq!(old.page_fingerprint(),digest);
    }
}

#[test]
fn staged_rollback_recovery_compaction_and_verified_restore_keep_primary_join_results() {
    use emilybase_query::execute;
    use emilybase_transactions::Database;
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let restored = temp.path().join("restored");
        let backup = temp.path().join("synthetic.backup");
        let mut db = Database::create(&source).unwrap();
        if compact {
            db.compact().unwrap();
        }
        execute(&mut db,"CREATE TABLE l(id INT PRIMARY KEY,link INT);CREATE TABLE r(label TEXT,id INT PRIMARY KEY);INSERT INTO l VALUES(1,9),(2,9),(3,NULL);INSERT INTO r VALUES('old',9)",&[]).unwrap();
        let sql = "SELECT a.id,b.label FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id";
        let old = db.view().unwrap().clone();
        let expected = query(&old, sql, &[]).unwrap();
        assert_eq!(
            expected.rows,
            vec![
                vec![Value::Integer(1), Value::Text("old".into())],
                vec![Value::Integer(2), Value::Text("old".into())]
            ]
        );
        let bytes = std::fs::read(source.join("redo.wal")).unwrap();
        let rolled=execute(&mut db,"BEGIN;UPDATE r SET label='staged' WHERE id=9;SELECT a.id,b.label FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id;ROLLBACK",&[]).unwrap();
        assert!(!rolled.committed);
        assert_eq!(
            rolled.results[1].rows,
            vec![
                vec![Value::Integer(1), Value::Text("staged".into())],
                vec![Value::Integer(2), Value::Text("staged".into())]
            ]
        );
        assert_eq!(std::fs::read(source.join("redo.wal")).unwrap(), bytes);
        assert_eq!(query(db.view().unwrap(), sql, &[]).unwrap(), expected);
        execute(
            &mut db,
            "UPDATE r SET label='committed' WHERE id=9;DELETE FROM l WHERE id=2",
            &[],
        )
        .unwrap();
        let committed = query(db.view().unwrap(), sql, &[]).unwrap();
        assert_eq!(
            committed.rows,
            vec![vec![Value::Integer(1), Value::Text("committed".into())]]
        );
        assert_eq!(query(&old, sql, &[]).unwrap(), expected);
        db.checkpoint().unwrap();
        if compact {
            db.compact().unwrap();
        }
        let database_id = db.database_id();
        let version =
            emilybase_transactions::recover_image(&db.committed_wal().unwrap(), Some(database_id))
                .unwrap()
                .wal_version;
        assert_eq!(version, if compact { 2 } else { 1 });
        let transaction = db.last_transaction();
        drop(db);
        let mut db = Database::open(&source).unwrap();
        assert_eq!(
            emilybase_transactions::recover_image(&db.committed_wal().unwrap(), Some(database_id))
                .unwrap()
                .wal_version,
            version
        );
        assert_eq!(query(db.view().unwrap(), sql, &[]).unwrap(), committed);
        emilybase_backup::create(&mut db, &backup).unwrap();
        emilybase_backup::inspect(&backup).unwrap();
        emilybase_backup::restore(&backup, &restored).unwrap();
        let mut copy = Database::open(&restored).unwrap();
        assert_eq!(copy.last_transaction(), transaction);
        assert_eq!(query(copy.view().unwrap(), sql, &[]).unwrap(), committed);
        execute(&mut copy, "INSERT INTO l VALUES(4,9)", &[]).unwrap();
        assert_eq!(query(copy.view().unwrap(), sql, &[]).unwrap().rows.len(), 2);
        assert_eq!(query(db.view().unwrap(), sql, &[]).unwrap(), committed);
        assert_eq!(query(&old, sql, &[]).unwrap(), expected);
    }
}

#[test]
fn sorted_primary_join_keeps_full_intermediate_byte_limit_before_limit() {
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "left_rows"), (2, "right_rows")] {
        add_table(
            &mut snapshot,
            id,
            name,
            &[
                ("id", DataType::Integer, false),
                ("link", DataType::Integer, false),
                ("hidden", DataType::Text, false),
            ],
            0,
        );
        for n in 0..1500 {
            insert(
                &mut snapshot,
                id,
                vec![
                    Value::Integer(n),
                    Value::Integer(n),
                    Value::Text("я".repeat(1536)),
                ],
            );
        }
    }
    let digest = snapshot.page_fingerprint();
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id ORDER BY b.link LIMIT 1",
            &[]
        ),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert_eq!(
        query(
            &snapshot,
            "SELECT a.id FROM left_rows AS a JOIN right_rows AS b ON a.link=b.id LIMIT 1",
            &[]
        )
        .unwrap()
        .rows,
        [vec![Value::Integer(0)]]
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
}
