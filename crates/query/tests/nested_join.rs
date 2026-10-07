use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{ExecutionError, query};
use proptest::prelude::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;

type Input = BTreeMap<i64, (Option<i64>, Option<bool>)>;
fn build(left: &Input, right: &Input) -> Snapshot {
    let mut view = Snapshot::empty().unwrap();
    for (table_id, name, input) in [(1, "a", left), (2, "b", right)] {
        view.apply(Event {
            table_id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                primary_key: 0,
                columns: [
                    ("id", DataType::Integer, false),
                    ("rank", DataType::Integer, true),
                    ("flag", DataType::Boolean, true),
                    ("body", DataType::Text, false),
                    ("data", DataType::Bytes, false),
                    ("f", DataType::Float, false),
                ]
                .into_iter()
                .map(|(name, data_type, nullable)| Column {
                    name: name.into(),
                    data_type,
                    nullable,
                })
                .collect(),
            }),
        })
        .unwrap();
        for (id, (rank, flag)) in input {
            view.apply(Event {
                table_id,
                kind: EventKind::Insert(vec![
                    Value::Integer(*id),
                    rank.map_or(Value::Null, Value::Integer),
                    flag.map_or(Value::Null, Value::Boolean),
                    Value::Text(format!("λ\0{id}")),
                    Value::Bytes(vec![0, 255, (*id as u8)]),
                    Value::Float(-0.0),
                ]),
            })
            .unwrap();
        }
    }
    view
}
fn rank_order(a: Option<i64>, b: Option<i64>, desc: bool, null_first: bool) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => {
            if null_first {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (Some(_), None) => {
            if null_first {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
        (Some(a), Some(b)) => {
            if desc {
                b.cmp(&a)
            } else {
                a.cmp(&b)
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn nullable_nonprimary_pairs_sort_projection_and_ranges_match_an_independent_model(
        left in prop::collection::btree_map(-20i64..20,(prop::option::of(-4i64..4),prop::option::of(any::<bool>())),0..20),
        right in prop::collection::btree_map(-20i64..20,(prop::option::of(-4i64..4),prop::option::of(any::<bool>())),0..20),
        mode in 0u8..4,lower in -25i64..25,upper in -25i64..25,
        limit in 0usize..41,desc in any::<bool>(),null_first in any::<bool>(),
    ) {
        let view=build(&left,&right);let before=view.page_fingerprint();
        let mut expected=Vec::new();
        for (a,(ar,af)) in &left { for (b,(br,bf)) in &right {
            let equal=ar.is_some() && br.is_some() && ar==br;
            let accepts=match mode {0|2=>equal,1=>equal || (a==b && *af==Some(true)),_=>equal && (*af==Some(true)||*bf==Some(true))};
            if accepts && *a>=lower && *a<upper && *bf!=Some(false) {expected.push((*a,*b,*br));}
        }}
        expected.sort_by(|(a,b,r),(c,d,s)|rank_order(*r,*s,desc,null_first).then_with(||c.cmp(a)).then_with(||b.cmp(d)));
        let expected=expected.into_iter().take(limit).map(|(a,b,_)|vec![Value::Integer(b),Value::Text(format!("λ\0{a}")),Value::Bytes(vec![0,255,b as u8]),Value::Float(-0.0),Value::Text(format!("λ\0{a}"))]).collect::<Vec<_>>();
        let on=match mode {0=>"a.rank=b.rank",1=>"a.rank=b.rank OR (a.id=b.id AND a.flag)",2=>"NOT (a.rank<>b.rank)",_=>"a.rank=b.rank AND (a.flag OR b.flag)"};
        let sql=format!("SELECT b.id,a.body,b.data,a.f,a.body AS repeated FROM a JOIN b ON {on} WHERE a.id >= $1 AND a.id < $2 AND (b.flag OR b.flag IS NULL) ORDER BY b.rank {} NULLS {},a.id DESC,b.id ASC LIMIT $3",if desc {"DESC"} else {"ASC"},if null_first {"FIRST"} else {"LAST"});
        let result=query(&view,&sql,&[Value::Integer(lower),Value::Integer(upper),Value::Integer(limit as i64)]).unwrap();
        prop_assert_eq!(&result.rows,&expected);
        prop_assert_eq!(result.columns,vec!["id","body","data","f","repeated"]);
        for row in result.rows {let Value::Float(f)=row[3] else {panic!("float fixture")};prop_assert_eq!(f.to_bits(),(-0.0f64).to_bits());}
        prop_assert_eq!(view.page_fingerprint(),before);
    }
}

fn wide(count: i64, payload: bool) -> Snapshot {
    let mut view = Snapshot::empty().unwrap();
    for (table_id, name) in [(1, "a"), (2, "b")] {
        view.apply(Event {
            table_id,
            kind: EventKind::Create(Schema {
                name: name.into(),
                columns: [
                    ("id", DataType::Integer),
                    ("rank", DataType::Integer),
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
        for id in 0..count {
            view.apply(Event {
                table_id,
                kind: EventKind::Insert(vec![
                    Value::Integer(id),
                    Value::Integer(0),
                    Value::Text(if payload {
                        "x".repeat(3000)
                    } else {
                        String::new()
                    }),
                ]),
            })
            .unwrap();
        }
    }
    view
}

#[test]
fn original_full_matched_row_byte_and_count_guards_still_refuse_small_sorted_output() {
    let view = wide(45, true);
    let before = view.page_fingerprint();
    assert!(matches!(
        query(
            &view,
            "SELECT a.id FROM a JOIN b ON TRUE ORDER BY a.id,b.id LIMIT 1",
            &[]
        ),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert_eq!(
        query(&view, "SELECT a.id FROM a JOIN b ON TRUE LIMIT 1", &[])
            .unwrap()
            .rows,
        [vec![Value::Integer(0)]]
    );
    assert_eq!(view.page_fingerprint(), before);
    let view = wide(101, false);
    assert!(matches!(
        query(
            &view,
            "SELECT a.id FROM a JOIN b ON TRUE ORDER BY a.id LIMIT 1",
            &[]
        ),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    let view = wide(225, false);
    assert!(matches!(
        query(&view, "SELECT a.id FROM a JOIN b ON FALSE LIMIT 1", &[]),
        Err(ExecutionError::Limit("query work"))
    ));
}

#[test]
fn long_text_point_keys_empty_inputs_and_self_joins_keep_physical_identity_and_order() {
    let mut view = Snapshot::empty().unwrap();
    view.apply(Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: "t".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
                Column {
                    name: "rank".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                },
            ],
            primary_key: 0,
        }),
    })
    .unwrap();
    let keys = [
        "a\0".into(),
        "b".repeat(256),
        "c".repeat(257),
        format!("λ{}", "d".repeat(3000)),
    ];
    for (id, key) in keys.iter().enumerate() {
        view.apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![
                Value::Text(key.clone()),
                Value::Integer(id as i64 % 2),
            ]),
        })
        .unwrap();
    }
    let before = view.page_fingerprint();
    let result = query(
        &view,
        "SELECT b.id FROM t AS a JOIN t AS b ON a.rank=b.rank WHERE a.id=$1 ORDER BY b.id",
        &[Value::Text(keys[3].clone())],
    )
    .unwrap();
    assert_eq!(
        result.rows,
        [
            vec![Value::Text(keys[1].clone())],
            vec![Value::Text(keys[3].clone())]
        ]
    );
    assert!(
        query(
            &view,
            "SELECT b.id FROM t AS a JOIN t AS b ON a.rank=b.rank WHERE a.id=$1",
            &[Value::Text("missing".into())]
        )
        .unwrap()
        .rows
        .is_empty()
    );
    assert!(
        query(
            &view,
            "SELECT b.id FROM t AS a JOIN t AS b ON a.rank=b.rank WHERE a.id >= 'z' AND a.id < 'a'",
            &[]
        )
        .unwrap()
        .rows
        .is_empty()
    );
    assert!(matches!(
        query(
            &view,
            "SELECT missing FROM t AS a JOIN t AS b ON FALSE LIMIT 0",
            &[]
        ),
        Err(ExecutionError::Column)
    ));
    assert_eq!(view.page_fingerprint(), before);
}

#[test]
fn both_wal_versions_keep_staged_join_reads_atomic_across_late_refusal_and_restore() {
    use emilybase_query::execute;
    use emilybase_transactions::Database;
    for compacted in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let mut database = Database::create(&path).unwrap();
        execute(&mut database,"CREATE TABLE a(id INT PRIMARY KEY,rank INT); CREATE TABLE b(id INT PRIMARY KEY,rank INT); INSERT INTO a VALUES(1,7),(2,8); INSERT INTO b VALUES(3,7),(4,8)",&[]).unwrap();
        if compacted {
            database.compact().unwrap();
        }
        let old = database.view().unwrap().clone();
        let sql = "SELECT a.id,b.id FROM a JOIN b ON a.rank=b.rank ORDER BY b.id";
        let expected = [
            vec![Value::Integer(1), Value::Integer(3)],
            vec![Value::Integer(2), Value::Integer(4)],
        ];
        assert_eq!(query(&old, sql, &[]).unwrap().rows, expected);
        let wal = database.committed_wal().unwrap();
        let transaction = database.last_transaction();
        let digest = database.view().unwrap().page_fingerprint();
        let rejected = format!("UPDATE a SET rank=8 WHERE id=1; {sql}; INSERT INTO b VALUES(3,9)");
        assert!(execute(&mut database, &rejected, &[]).is_err());
        assert_eq!(database.committed_wal().unwrap(), wal);
        assert_eq!(database.last_transaction(), transaction);
        assert_eq!(database.view().unwrap().page_fingerprint(), digest);
        assert_eq!(
            query(database.view().unwrap(), sql, &[]).unwrap().rows,
            expected
        );
        let rollback = format!("BEGIN; UPDATE a SET rank=8 WHERE id=1; {sql}; ROLLBACK");
        execute(&mut database, &rollback, &[]).unwrap();
        assert_eq!(database.committed_wal().unwrap(), wal);
        let committed =
            format!("UPDATE a SET rank=8 WHERE id=1; UPDATE b SET rank=8 WHERE id=3; {sql}");
        let result = execute(&mut database, &committed, &[]).unwrap();
        let next = [
            vec![Value::Integer(1), Value::Integer(3)],
            vec![Value::Integer(2), Value::Integer(3)],
            vec![Value::Integer(1), Value::Integer(4)],
            vec![Value::Integer(2), Value::Integer(4)],
        ];
        assert_eq!(result.results[2].rows, next);
        assert_eq!(query(&old, sql, &[]).unwrap().rows, expected);
        let archive = dir.path().join("synthetic.backup");
        assert_eq!(
            emilybase_backup::create(&mut database, &archive)
                .unwrap()
                .wal_version,
            if compacted { 2 } else { 1 }
        );
        let target = dir.path().join("restored");
        emilybase_backup::restore(&archive, &target).unwrap();
        let restored = Database::open(&target).unwrap();
        assert_eq!(
            query(restored.view().unwrap(), sql, &[]).unwrap().rows,
            next
        );
        drop(database);
        let reopened = Database::open(&path).unwrap();
        assert_eq!(
            query(reopened.view().unwrap(), sql, &[]).unwrap().rows,
            next
        );
    }
}
