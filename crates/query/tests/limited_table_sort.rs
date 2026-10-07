use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::query;

fn wide(count: i64) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: [
                    ("rank", DataType::Integer),
                    ("id", DataType::Integer),
                    ("hidden", DataType::Text),
                ]
                .into_iter()
                .map(|(name, data_type)| Column {
                    name: name.into(),
                    data_type,
                    nullable: false,
                })
                .collect(),
                primary_key: 1,
            }),
        })
        .unwrap();
    for id in 0..count {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(count - 1 - id),
                    Value::Integer(id),
                    Value::Text("x".repeat(3072)),
                ]),
            })
            .unwrap();
    }
    snapshot
}
#[test]
fn small_nonprimary_order_limit_does_not_materialize_every_wide_source_row() {
    let snapshot = wide(2800);
    let digest = snapshot.page_fingerprint();
    let result = query(&snapshot, "SELECT id FROM t ORDER BY rank ASC LIMIT 2", &[]).unwrap();
    assert_eq!(
        result.rows,
        [vec![Value::Integer(2799)], vec![Value::Integer(2798)]]
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
}

use emilybase_catalog::Key;
use emilybase_query::{ExecutionError, execute, explain};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[test]
fn every_orderable_type_and_stable_tie_matches_explicit_expected_source_ids() {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: [
                    ("rank", DataType::Integer),
                    ("id", DataType::Integer),
                    ("flag", DataType::Boolean),
                    ("text", DataType::Text),
                    ("bytes", DataType::Bytes),
                    ("f", DataType::Float),
                ]
                .into_iter()
                .map(|(name, data_type)| Column {
                    name: name.into(),
                    data_type,
                    nullable: name != "id",
                })
                .collect(),
                primary_key: 1,
            }),
        })
        .unwrap();
    let rows = [
        vec![
            Value::Integer(2),
            Value::Integer(1),
            Value::Boolean(false),
            Value::Text("é".into()),
            Value::Bytes(vec![0]),
            Value::Float(-3.0),
        ],
        vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Boolean(true),
            Value::Text("e\u{301}".into()),
            Value::Bytes(vec![0, 255]),
            Value::Float(-0.0),
        ],
        vec![
            Value::Null,
            Value::Integer(3),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ],
        vec![
            Value::Integer(1),
            Value::Integer(4),
            Value::Boolean(true),
            Value::Text("я\0".into()),
            Value::Bytes(vec![255]),
            Value::Float(0.0),
        ],
        vec![
            Value::Integer(0),
            Value::Integer(5),
            Value::Boolean(false),
            Value::Text(String::new()),
            Value::Bytes(vec![]),
            Value::Float(7.0),
        ],
    ];
    for index in [4, 2, 0, 3, 1] {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(rows[index].clone()),
            })
            .unwrap();
    }
    for (order, ids) in [
        ("rank ASC NULLS LAST", [5, 2, 4, 1, 3]),
        ("rank DESC NULLS FIRST", [3, 1, 2, 4, 5]),
        ("flag ASC NULLS LAST", [1, 5, 2, 4, 3]),
        ("flag DESC NULLS FIRST", [3, 2, 4, 1, 5]),
        ("f ASC NULLS FIRST", [3, 1, 2, 4, 5]),
        ("f DESC NULLS LAST", [5, 2, 4, 1, 3]),
        ("text ASC NULLS LAST", [5, 2, 1, 4, 3]),
        ("text DESC NULLS FIRST", [3, 4, 1, 2, 5]),
        ("bytes ASC NULLS FIRST", [3, 5, 1, 2, 4]),
        ("bytes DESC NULLS LAST", [4, 2, 1, 5, 3]),
    ] {
        for limit in [0, 1, 2, 3, 5, 10000] {
            let sql = format!("SELECT id,f FROM t ORDER BY {order} LIMIT {limit}");
            let result = query(&snapshot, &sql, &[]).unwrap();
            let expected = ids
                .iter()
                .take(limit)
                .map(|id| vec![Value::Integer(*id), rows[*id as usize - 1][5].clone()])
                .collect::<Vec<_>>();
            assert_eq!(result.rows, expected, "{sql}");
            for (actual, expected) in result.rows.iter().zip(&expected) {
                if let (Value::Float(a), Value::Float(b)) = (&actual[1], &expected[1]) {
                    assert_eq!(a.to_bits(), b.to_bits());
                }
            }
        }
    }
    assert_eq!(
        query(
            &snapshot,
            "SELECT id FROM t ORDER BY rank ASC NULLS LAST,id DESC LIMIT 3",
            &[]
        )
        .unwrap()
        .rows,
        [
            vec![Value::Integer(5)],
            vec![Value::Integer(4)],
            vec![Value::Integer(2)]
        ]
    );
}

#[test]
fn necessary_points_ranges_integer_extremes_and_empty_binding_remain_exact() {
    let snapshot = wide(600);
    for (condition, expected) in [
        ("id=550", vec![550]),
        ("550=id", vec![550]),
        ("id>=500 AND id<520", vec![519, 518]),
        ("500<=id AND 520>id", vec![519, 518]),
        ("id>=500 AND id<500", vec![]),
        ("id=550 AND id=551", vec![]),
        ("id=99999", vec![]),
    ] {
        let sql = format!("SELECT x.id FROM t AS x WHERE {condition} ORDER BY x.rank LIMIT 2");
        assert_eq!(
            query(&snapshot, &sql, &[]).unwrap().rows,
            expected
                .into_iter()
                .map(|id| vec![Value::Integer(id)])
                .collect::<Vec<_>>()
        );
    }
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "rank".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                ],
                primary_key: 1,
            }),
        })
        .unwrap();
    for (id, rank) in [(i64::MIN, 5), (-1, 4), (0, 3), (1, 2), (i64::MAX, 1)] {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(rank), Value::Integer(id)]),
            })
            .unwrap();
    }
    assert_eq!(
        query(
            &snapshot,
            "SELECT id FROM t WHERE id>=$1 ORDER BY rank LIMIT 2",
            &[Value::Integer(i64::MIN)]
        )
        .unwrap()
        .rows,
        [vec![Value::Integer(i64::MAX)], vec![Value::Integer(1)]]
    );
    assert!(
        query(
            &snapshot,
            "SELECT id FROM t WHERE id>$1 ORDER BY rank LIMIT 1",
            &[Value::Integer(i64::MAX)]
        )
        .unwrap()
        .rows
        .is_empty()
    );
    for sql in [
        "SELECT missing FROM t ORDER BY rank LIMIT 0",
        "SELECT id FROM t ORDER BY missing LIMIT 0",
        "SELECT id FROM t WHERE rank='wrong' ORDER BY rank LIMIT 0",
        "SELECT id FROM t WHERE id>1 AND id<1 ORDER BY missing",
    ] {
        assert!(query(&snapshot, sql, &[]).is_err(), "{sql}");
    }
}

#[test]
fn long_utf8_primary_keys_old_views_and_nonprimary_sort_stay_validated() {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![
                    Column {
                        name: "rank".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    },
                    Column {
                        name: "id".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
                primary_key: 1,
            }),
        })
        .unwrap();
    let mut keys = vec!["".into(), "я\0".into(), "e\u{301}".into(), "é".into()];
    for n in [255, 256, 257, 3072] {
        keys.push("x".repeat(n));
    }
    for (rank, key) in keys.iter().enumerate() {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Integer(rank as i64),
                    Value::Text(key.clone()),
                ]),
            })
            .unwrap();
    }
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    assert_eq!(
        query(
            &snapshot,
            "SELECT id FROM t ORDER BY rank DESC LIMIT 2",
            &[]
        )
        .unwrap()
        .rows,
        keys.iter()
            .rev()
            .take(2)
            .map(|key| vec![Value::Text(key.clone())])
            .collect::<Vec<_>>()
    );
    for key in &keys {
        assert_eq!(
            query(
                &snapshot,
                "SELECT id FROM t WHERE id=$1 ORDER BY rank LIMIT 2",
                &[Value::Text(key.clone())]
            )
            .unwrap()
            .rows,
            [vec![Value::Text(key.clone())]]
        );
    }
    let lower = "x".repeat(255);
    let upper = "я\0".to_owned();
    let mut expected = keys
        .iter()
        .enumerate()
        .filter(|(_, key)| *key >= &lower && *key < &upper)
        .collect::<Vec<_>>();
    expected.reverse();
    expected.truncate(3);
    assert_eq!(
        query(
            &snapshot,
            "SELECT id FROM t WHERE id>=$1 AND id<$2 ORDER BY rank DESC LIMIT 3",
            &[Value::Text(lower), Value::Text(upper)]
        )
        .unwrap()
        .rows,
        expected
            .iter()
            .map(|(_, key)| vec![Value::Text((*key).clone())])
            .collect::<Vec<_>>()
    );
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Text(keys.last().unwrap().clone())),
        })
        .unwrap();
    assert_eq!(old.page_fingerprint(), digest);
    assert_eq!(
        query(&old, "SELECT id FROM t ORDER BY rank DESC LIMIT 1", &[])
            .unwrap()
            .rows,
        [vec![Value::Text(keys.last().unwrap().clone())]]
    );
}

#[test]
fn large_retained_output_and_shared_script_caps_remain_active() {
    let snapshot = wide(2800);
    let digest = snapshot.page_fingerprint();
    assert!(matches!(
        query(&snapshot, "SELECT id FROM t ORDER BY rank LIMIT 2800", &[]),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert!(matches!(
        query(
            &snapshot,
            "SELECT hidden,hidden FROM t ORDER BY rank LIMIT 1400",
            &[]
        ),
        Err(ExecutionError::Limit("output bytes"))
    ));
    assert!(
        query(
            &snapshot,
            "SELECT id FROM t WHERE FALSE ORDER BY rank LIMIT 1",
            &[]
        )
        .unwrap()
        .rows
        .is_empty()
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_nullable_sort_limits_match_independent_primary_order_model(
        entries in prop::collection::vec((any::<i16>(),prop::option::of(-5i64..6)),0..81),
        lower in -30000i64..30000,upper in -30000i64..30000,limit in 0usize..82,
        descending in any::<bool>(),nulls_first in any::<bool>(),
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        snapshot.apply(Event{table_id:1,kind:EventKind::Create(Schema{name:"t".into(),columns:vec![Column{name:"rank".into(),data_type:DataType::Integer,nullable:true},Column{name:"id".into(),data_type:DataType::Integer,nullable:false}],primary_key:1})}).unwrap();
        let model=entries.into_iter().map(|(id,rank)|(i64::from(id),rank)).collect::<BTreeMap<_,_>>();
        for (id,rank) in &model {snapshot.apply(Event{table_id:1,kind:EventKind::Insert(vec![rank.map_or(Value::Null,Value::Integer),Value::Integer(*id)])}).unwrap();}
        let mut expected=model.iter().filter(|(id,_)|**id>=lower&&**id<upper).map(|(id,rank)|(*id,*rank)).collect::<Vec<_>>();
        expected.sort_by(|a,b|match(a.1,b.1){(None,None)=>Ordering::Equal,(None,_)=>if nulls_first{Ordering::Less}else{Ordering::Greater},(_,None)=>if nulls_first{Ordering::Greater}else{Ordering::Less},(Some(a),Some(b))=>if descending{b.cmp(&a)}else{a.cmp(&b)}});expected.truncate(limit);
        let sql=format!("SELECT id,rank FROM t WHERE id>=$1 AND id<$2 ORDER BY rank {} NULLS {} LIMIT {limit}",if descending{"DESC"}else{"ASC"},if nulls_first{"FIRST"}else{"LAST"});
        let old=snapshot.clone();let digest=old.page_fingerprint();
        prop_assert_eq!(query(&snapshot,&sql,&[Value::Integer(lower),Value::Integer(upper)]).unwrap().rows,expected.into_iter().map(|(id,rank)|vec![Value::Integer(id),rank.map_or(Value::Null,Value::Integer)]).collect::<Vec<_>>());
        prop_assert_eq!(snapshot.page_fingerprint(),digest);prop_assert_eq!(old.page_fingerprint(),digest);
    }
}

#[test]
fn both_wals_keep_atomic_sorting_staged_changes_and_verified_restore() {
    for compact in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source");
        let restored = directory.path().join("restored");
        let archive = directory.path().join("copy.backup");
        let mut db = Database::create(&path).unwrap();
        if compact {
            db.compact().unwrap();
        }
        execute(&mut db,"CREATE TABLE t(rank INT,id INT PRIMARY KEY,payload TEXT);INSERT INTO t VALUES(3,1,'old'),(1,2,'two'),(NULL,3,NULL),(1,4,'four')",&[]).unwrap();
        let read = "SELECT id,payload FROM t ORDER BY rank ASC NULLS LAST LIMIT 2";
        let old = db.view().unwrap().clone();
        let digest = old.page_fingerprint();
        let expected = query(&old, read, &[]).unwrap();
        let wal = db.committed_wal().unwrap();
        let transaction = db.last_transaction();
        assert_eq!(explain(&old, read, &[]).unwrap().access, "scan");
        let rolled = execute(
            &mut db,
            &format!("BEGIN;UPDATE t SET rank=-1,payload='staged' WHERE id=1;{read};ROLLBACK"),
            &[],
        )
        .unwrap();
        assert!(!rolled.committed);
        assert_eq!(
            rolled.results[1].rows[0],
            [Value::Integer(1), Value::Text("staged".into())]
        );
        assert_eq!(db.committed_wal().unwrap(), wal);
        assert_eq!(db.last_transaction(), transaction);
        assert!(
            execute(
                &mut db,
                &format!("UPDATE t SET rank=-1 WHERE id=1;{read};SELECT missing FROM t"),
                &[]
            )
            .is_err()
        );
        assert_eq!(db.committed_wal().unwrap(), wal);
        execute(&mut db, "UPDATE t SET rank=-1 WHERE id=1", &[]).unwrap();
        let committed = query(db.view().unwrap(), read, &[]).unwrap();
        assert_eq!(
            committed.rows[0],
            [Value::Integer(1), Value::Text("old".into())]
        );
        assert_eq!(query(&old, read, &[]).unwrap(), expected);
        assert_eq!(old.page_fingerprint(), digest);
        db.checkpoint().unwrap();
        drop(db);
        let mut db = Database::open(&path).unwrap();
        assert_eq!(query(db.view().unwrap(), read, &[]).unwrap(), committed);
        assert_eq!(
            emilybase_transactions::recover_image(
                &db.committed_wal().unwrap(),
                Some(db.database_id())
            )
            .unwrap()
            .wal_version,
            if compact { 2 } else { 1 }
        );
        emilybase_backup::create(&mut db, &archive).unwrap();
        emilybase_backup::inspect(&archive).unwrap();
        emilybase_backup::restore(&archive, &restored).unwrap();
        let mut copy = Database::open(restored).unwrap();
        assert_eq!(query(copy.view().unwrap(), read, &[]).unwrap(), committed);
        execute(&mut copy, "INSERT INTO t VALUES(-2,5,'independent')", &[]).unwrap();
        assert_eq!(
            query(copy.view().unwrap(), read, &[]).unwrap().rows[0],
            [Value::Integer(5), Value::Text("independent".into())]
        );
        assert_eq!(query(db.view().unwrap(), read, &[]).unwrap(), committed);
    }
}
