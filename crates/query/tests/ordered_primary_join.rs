use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

fn wide() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    for (id, name) in [(1, "l"), (2, "r")] {
        snapshot
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(Schema {
                    name: name.into(),
                    columns: [
                        ("id", DataType::Integer),
                        ("link", DataType::Integer),
                        ("hidden", DataType::Text),
                    ]
                    .into_iter()
                    .map(|(name, data_type)| Column {
                        name: name.into(),
                        data_type,
                        nullable: false,
                    })
                    .collect(),
                    primary_key: 0,
                }),
            })
            .unwrap();
        for n in 0..1500 {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(n),
                        Value::Integer(n),
                        Value::Text("x".repeat(3072)),
                    ]),
                })
                .unwrap();
        }
    }
    snapshot
}
#[test]
fn ordered_unique_left_primary_join_stops_before_cloning_all_hidden_rows() {
    let snapshot = wide();
    let digest = snapshot.page_fingerprint();
    let result = query(
        &snapshot,
        "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 2",
        &[],
    )
    .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Value::Integer(1499); 2], vec![Value::Integer(1498); 2]]
    );
    // A narrow projection may return all IDs without retaining every hidden field.
    let all = query(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 1500",
        &[],
    )
    .unwrap();
    assert_eq!(all.rows.len(), 1500);
    assert_eq!(all.rows.first().unwrap(), &vec![Value::Integer(1499)]);
    assert_eq!(all.rows.last().unwrap(), &vec![Value::Integer(0)]);
    assert_eq!(snapshot.page_fingerprint(), digest);
}

use emilybase_catalog::{Key, Row};
use emilybase_query::{ExecutionError, execute, explain};
use proptest::prelude::*;

fn table(
    snapshot: &mut Snapshot,
    id: u64,
    name: &str,
    fields: &[(&str, DataType, bool)],
    primary: u16,
) {
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                columns: fields
                    .iter()
                    .map(|(name, data_type, nullable)| Column {
                        name: (*name).into(),
                        data_type: *data_type,
                        nullable: *nullable,
                    })
                    .collect(),
                primary_key: primary,
            }),
        })
        .unwrap();
}
fn row(snapshot: &mut Snapshot, id: u64, values: Row) {
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Insert(values),
        })
        .unwrap();
}
fn small() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    table(
        &mut snapshot,
        1,
        "l",
        &[
            ("link", DataType::Integer, true),
            ("id", DataType::Integer, false),
            ("n", DataType::Integer, true),
        ],
        1,
    );
    table(
        &mut snapshot,
        2,
        "r",
        &[
            ("n", DataType::Integer, true),
            ("id", DataType::Integer, false),
        ],
        1,
    );
    for id in 0..60 {
        row(
            &mut snapshot,
            1,
            vec![
                if id % 7 == 0 {
                    Value::Null
                } else {
                    Value::Integer(id % 9)
                },
                Value::Integer(id),
                if id % 5 == 0 {
                    Value::Null
                } else {
                    Value::Integer(id % 4)
                },
            ],
        );
    }
    for id in 0..8 {
        row(
            &mut snapshot,
            2,
            vec![Value::Integer(id % 3), Value::Integer(id)],
        );
    }
    snapshot
}
fn parity(snapshot: &Snapshot, condition: &str, order: &str, limit: usize, bindings: &[Value]) {
    let sql = format!(
        "SELECT a.id,b.id,a.n FROM l AS a JOIN r AS b ON a.link=b.id WHERE {condition} ORDER BY {order} LIMIT {limit}"
    );
    let reference = format!(
        "SELECT a.id,b.id,a.n FROM l AS a JOIN r AS b ON a.link=b.id OR FALSE WHERE {condition} ORDER BY {order} LIMIT {limit}"
    );
    assert_eq!(
        explain(snapshot, &sql, bindings).unwrap().access,
        "primary_join"
    );
    assert_eq!(
        query(snapshot, &sql, bindings).unwrap(),
        query(snapshot, &reference, bindings).unwrap()
    );
}

#[test]
fn unique_left_order_prefix_keeps_secondary_null_rules_and_counts_matches() {
    let snapshot = small();
    for order in [
        "a.id ASC",
        "a.id DESC",
        "a.id DESC,a.n ASC NULLS FIRST,b.n DESC",
        "a.id ASC NULLS LAST,b.id DESC",
    ] {
        for limit in [0, 1, 2, 19, 10000] {
            parity(&snapshot, "TRUE", order, limit, &[]);
            parity(&snapshot, "b.n>=1 AND a.n IS NOT NULL", order, limit, &[]);
        }
    }
    let result=query(&snapshot,"SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE b.id=3 ORDER BY a.id DESC LIMIT 2",&[]).unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Value::Integer(57)], vec![Value::Integer(48)]]
    );
}

#[test]
fn left_points_and_necessary_ranges_preserve_complete_join_and_where() {
    let snapshot = small();
    for condition in [
        "a.id=12",
        "12=a.id",
        "a.id=$1",
        "a.id=12 AND a.id=13",
        "a.id>=12 AND a.id<20",
        "12<=a.id AND 20>a.id",
        "a.id>=12 AND a.id<12",
        "a.id>$1 AND a.id<30 AND b.n=1",
        "a.id>=10 AND (b.n=1 OR a.n IS NULL)",
        "a.id>=10 AND NOT(a.n=3)",
        "a.id<10 OR a.id>50",
    ] {
        for order in ["a.id ASC", "a.id DESC,a.n DESC", "b.id DESC,a.id ASC"] {
            parity(&snapshot, condition, order, 3, &[Value::Integer(12)]);
        }
    }
    assert!(query(&snapshot,"SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id=10000 ORDER BY a.id DESC LIMIT 2",&[]).unwrap().rows.is_empty());
}

#[test]
fn right_ordering_or_nonprimary_first_key_keeps_sort_ties_and_retained_limit() {
    let snapshot = small();
    for order in [
        "b.id DESC,a.id DESC",
        "a.n ASC NULLS LAST,a.id DESC",
        "b.n DESC NULLS FIRST,a.id ASC",
    ] {
        parity(&snapshot, "TRUE", order, 8, &[]);
    }
    let snapshot = wide();
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.link DESC,a.id LIMIT 1500",
            &[]
        ),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.hidden,a.id LIMIT 1500",
            &[]
        ),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
}

#[test]
fn mixed_short_long_utf8_primary_order_ranges_points_and_old_views_are_exact() {
    let mut snapshot = Snapshot::empty().unwrap();
    table(
        &mut snapshot,
        1,
        "l",
        &[
            ("id", DataType::Text, false),
            ("link", DataType::Integer, true),
            ("n", DataType::Integer, true),
        ],
        0,
    );
    table(
        &mut snapshot,
        2,
        "r",
        &[("id", DataType::Integer, false)],
        0,
    );
    row(&mut snapshot, 2, vec![Value::Integer(1)]);
    let mut keys = vec![
        String::new(),
        "\0".into(),
        "a".into(),
        "a\0".into(),
        "b".into(),
        "é".into(),
        "é".into(),
        "я\0".into(),
        "a".repeat(255),
        "a".repeat(256),
        "a".repeat(257),
        format!("a{}", "z".repeat(3071)),
    ];
    for (i, key) in keys.iter().enumerate() {
        row(
            &mut snapshot,
            1,
            vec![
                Value::Text(key.clone()),
                Value::Integer(1),
                Value::Integer(i as i64),
            ],
        );
    }
    keys.sort();
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    for descending in [false, true] {
        let direction = if descending { "DESC" } else { "ASC" };
        let mut expected = keys.clone();
        if descending {
            expected.reverse();
        }
        expected.truncate(5);
        let sql = format!(
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id {direction},b.id DESC LIMIT 5"
        );
        assert_eq!(
            query(&snapshot, &sql, &[]).unwrap().rows,
            expected
                .iter()
                .map(|k| vec![Value::Text(k.clone())])
                .collect::<Vec<_>>()
        );
    }
    for bound in ["a", "a\0", "é"] {
        let expected: Vec<_> = keys
            .iter()
            .rev()
            .filter(|key| key.as_str() > bound)
            .take(4)
            .map(|k| vec![Value::Text(k.clone())])
            .collect();
        assert_eq!(query(&snapshot,"SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>$1 ORDER BY a.id DESC LIMIT 4",&[Value::Text(bound.into())]).unwrap().rows,expected);
    }
    for key in keys.iter().filter(|k| k.len() >= 255) {
        assert_eq!(query(&snapshot,"SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id=$1 ORDER BY a.id DESC LIMIT 2",&[Value::Text(key.clone())]).unwrap().rows,[vec![Value::Text(key.clone())]]);
    }
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Text(keys.last().unwrap().clone())),
        })
        .unwrap();
    let sql = "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 1";
    assert_ne!(
        query(&snapshot, sql, &[]).unwrap(),
        query(&old, sql, &[]).unwrap()
    );
    assert_eq!(old.page_fingerprint(), digest);
}

#[test]
fn integer_extremes_contradictions_empty_ranges_and_zero_limit_keep_validation() {
    let mut snapshot = Snapshot::empty().unwrap();
    table(
        &mut snapshot,
        1,
        "l",
        &[
            ("id", DataType::Integer, false),
            ("link", DataType::Integer, false),
        ],
        0,
    );
    table(
        &mut snapshot,
        2,
        "r",
        &[("id", DataType::Integer, false)],
        0,
    );
    row(&mut snapshot, 2, vec![Value::Integer(1)]);
    for id in [i64::MIN, -1, 0, 1, i64::MAX] {
        row(
            &mut snapshot,
            1,
            vec![Value::Integer(id), Value::Integer(1)],
        );
    }
    let sql = "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>=$1 ORDER BY a.id DESC LIMIT 2";
    assert_eq!(
        query(&snapshot, sql, &[Value::Integer(i64::MIN)])
            .unwrap()
            .rows,
        [vec![Value::Integer(i64::MAX)], vec![Value::Integer(1)]]
    );
    let sql = "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>$1 ORDER BY a.id DESC LIMIT 2";
    assert!(
        query(&snapshot, sql, &[Value::Integer(i64::MAX)])
            .unwrap()
            .rows
            .is_empty()
    );
    for sql in [
        "SELECT a.missing FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>1 AND a.id<1 LIMIT 0",
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.missing LIMIT 0",
    ] {
        assert!(matches!(
            query(&snapshot, sql, &[]),
            Err(ExecutionError::Column)
        ));
    }
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>1 AND a.id<1 AND b.id='wrong'",
            &[]
        ),
        Err(ExecutionError::Type)
    ));
}

#[test]
fn repeated_ordered_join_scripts_stay_bounded_and_leave_both_wals_unchanged() {
    use emilybase_transactions::Database;
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("synthetic");
        let mut database = Database::create(&path).unwrap();
        if compact {
            database.compact().unwrap();
        }
        execute(&mut database,"CREATE TABLE l(id INT PRIMARY KEY,link INT);CREATE TABLE r(id INT PRIMARY KEY);INSERT INTO r VALUES(1)",&[]).unwrap();
        for start in [0, 200, 400] {
            let values = (start..start + 200)
                .map(|n| format!("({n},1)"))
                .collect::<Vec<_>>()
                .join(",");
            execute(
                &mut database,
                &format!("INSERT INTO l VALUES {values}"),
                &[],
            )
            .unwrap();
        }
        let old = database.view().unwrap().clone();
        let digest = old.page_fingerprint();
        let before = database.committed_wal().unwrap();
        let transaction = database.last_transaction();
        let script =
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 1;"
                .repeat(64);
        let report = execute(&mut database, &script, &[]).unwrap();
        // Successful implicit read scripts report completion without a new WAL transaction.
        assert!(report.committed);
        assert_eq!(report.transaction, transaction);
        assert_eq!(report.results.len(), 64);
        assert!(
            report
                .results
                .iter()
                .all(|r| r.rows == vec![vec![Value::Integer(599)]])
        );
        assert_eq!(database.committed_wal().unwrap(), before);
        assert_eq!(old.page_fingerprint(), digest);
        database.checkpoint().unwrap();
        drop(database);
        let mut database = Database::open(&path).unwrap();
        assert_eq!(
            execute(&mut database, &script, &[])
                .unwrap()
                .results
                .last()
                .unwrap()
                .rows,
            [vec![Value::Integer(599)]]
        );
        assert_eq!(database.committed_wal().unwrap(), before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_source_ranges_orders_and_match_limits_agree_with_independent_join(
        left in prop::collection::btree_map(-50i64..50,prop::option::of(-8i64..16),0..48),
        right in prop::collection::btree_set(-8i64..16,0..20),
        lower in -60i64..60, upper in -60i64..60, descending in any::<bool>(), limit in 0usize..20,
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        table(&mut snapshot,1,"l",&[("id",DataType::Integer,false),("link",DataType::Integer,true),("n",DataType::Integer,true)],0);
        table(&mut snapshot,2,"r",&[("id",DataType::Integer,false)],0);
        for (id,link) in &left {row(&mut snapshot,1,vec![Value::Integer(*id),link.map_or(Value::Null,Value::Integer),Value::Integer(*id)]);}
        for id in &right {row(&mut snapshot,2,vec![Value::Integer(*id)]);}
        let mut expected:Vec<_>=left.iter().filter_map(|(id,link)| {
            let target=(*link)?;
            (*id>=lower && *id<upper && right.contains(&target) && target>=0).then_some(vec![Value::Integer(*id),Value::Integer(target)])
        }).collect();
        if descending {expected.reverse();}
        expected.truncate(limit);
        let direction=if descending {"DESC"} else {"ASC"};
        let sql=format!("SELECT a.id,b.id FROM l AS a JOIN r AS b ON b.id=a.link WHERE a.id>=$1 AND a.id<$2 AND b.id>=0 ORDER BY a.id {direction},a.n DESC LIMIT $3");
        let bindings=[Value::Integer(lower),Value::Integer(upper),Value::Integer(limit as i64)];
        prop_assert_eq!(query(&snapshot,&sql,&bindings).unwrap().rows,expected);
        let fallback=sql.replace("ON b.id=a.link WHERE","ON b.id=a.link OR FALSE WHERE");
        prop_assert_eq!(query(&snapshot,&sql,&bindings).unwrap(),query(&snapshot,&fallback,&bindings).unwrap());
        prop_assert_eq!(explain(&snapshot,&sql,&bindings).unwrap().access,"primary_join");
    }
}

#[test]
fn full_global_row_capacity_keeps_many_to_one_ordered_join_and_original_limits() {
    let mut snapshot = Snapshot::empty().unwrap();
    table(
        &mut snapshot,
        1,
        "l",
        &[
            ("id", DataType::Integer, false),
            ("link", DataType::Integer, false),
        ],
        0,
    );
    table(
        &mut snapshot,
        2,
        "r",
        &[("id", DataType::Integer, false)],
        0,
    );
    row(&mut snapshot, 2, vec![Value::Integer(1)]);
    for id in 0..9999 {
        row(
            &mut snapshot,
            1,
            vec![Value::Integer(id), Value::Integer(1)],
        );
    }
    assert_eq!(snapshot.row_count(), 10000);
    let digest = snapshot.page_fingerprint();
    let sql = "SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY a.id DESC LIMIT 2";
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap().rows,
        [
            vec![Value::Integer(9998), Value::Integer(1)],
            vec![Value::Integer(9997), Value::Integer(1)]
        ]
    );
    assert_eq!(query(&snapshot,"SELECT a.id,b.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>=9997 ORDER BY a.id DESC LIMIT 2",&[]).unwrap().rows,query(&snapshot,sql,&[]).unwrap().rows);
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert!(
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(10000), Value::Integer(1)])
            })
            .is_err()
    );
    assert_eq!(snapshot.row_count(), 10000);
    assert_eq!(snapshot.page_fingerprint(), digest);
}
