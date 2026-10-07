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
        for n in 0..1500 {
            snapshot
                .apply(Event {
                    table_id: id,
                    kind: EventKind::Insert(vec![
                        Value::Integer(n),
                        Value::Integer(n),
                        Value::Integer(1499 - n),
                        Value::Text("x".repeat(3072)),
                    ]),
                })
                .unwrap();
        }
    }
    snapshot
}

#[test]
fn small_right_sort_limit_keeps_only_best_wide_join_matches() {
    let snapshot = wide();
    let digest = snapshot.page_fingerprint();
    let result = query(
        &snapshot,
        "SELECT a.id,b.rank FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank ASC LIMIT 2",
        &[],
    )
    .unwrap();
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Integer(1499), Value::Integer(0)],
            vec![Value::Integer(1498), Value::Integer(1)]
        ]
    );
    assert_eq!(snapshot.page_fingerprint(), digest);
}

use emilybase_catalog::{Key, Row};
use emilybase_query::{ExecutionError, execute, explain};
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::cmp::Ordering;

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
fn insert(snapshot: &mut Snapshot, id: u64, row: Row) {
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Insert(row),
        })
        .unwrap();
}
fn typed() -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    let fields = [
        ("id", DataType::Integer, false),
        ("link", DataType::Integer, true),
        ("rank", DataType::Integer, true),
        ("flag", DataType::Boolean, true),
        ("score", DataType::Float, true),
        ("text", DataType::Text, true),
        ("bytes", DataType::Bytes, true),
    ];
    for (table_id, name, count) in [(1, "l", 200), (2, "r", 20)] {
        table(&mut snapshot, table_id, name, &fields, 0);
        for id in 0..count {
            insert(
                &mut snapshot,
                table_id,
                vec![
                    Value::Integer(id),
                    if id % 11 == 0 {
                        Value::Null
                    } else {
                        Value::Integer(id % 23)
                    },
                    if id % 5 == 0 {
                        Value::Null
                    } else {
                        Value::Integer(id % 4 - 2)
                    },
                    if id % 3 == 0 {
                        Value::Null
                    } else {
                        Value::Boolean(id % 2 == 0)
                    },
                    if id % 13 == 0 {
                        Value::Null
                    } else {
                        Value::Float([0.0, -0.0, -2.5, 4.125][id as usize % 4])
                    },
                    if id % 7 == 0 {
                        Value::Null
                    } else {
                        Value::Text(["", "я\0", "é", "e\u{301}", "λ"][id as usize % 5].into())
                    },
                    if id % 6 == 0 {
                        Value::Null
                    } else {
                        Value::Bytes(vec![(id % 4) as u8, 0, 255])
                    },
                ],
            );
        }
    }
    snapshot
}
fn parity(snapshot: &Snapshot, filter: &str, order: &str, limit: usize) {
    let sql = format!(
        "SELECT a.id,b.id,b.score,b.text,b.bytes FROM l AS a JOIN r AS b ON a.link=b.id WHERE {filter} ORDER BY {order} LIMIT {limit}"
    );
    let reference = sql.replace("ON a.link=b.id WHERE", "ON a.link=b.id OR FALSE WHERE");
    assert_eq!(explain(snapshot, &sql, &[]).unwrap().access, "primary_join");
    let actual = query(snapshot, &sql, &[]).unwrap();
    let full = query(snapshot, &reference, &[]).unwrap();
    assert_eq!(actual, full);
    for (a, b) in actual.rows.iter().zip(&full.rows) {
        if let (Value::Float(a), Value::Float(b)) = (&a[2], &b[2]) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}

#[test]
fn all_sort_types_null_rules_directions_and_stable_ties_match_full_reference() {
    let snapshot = typed();
    let digest = snapshot.page_fingerprint();
    for order in [
        "b.rank ASC NULLS FIRST",
        "b.rank DESC NULLS LAST",
        "b.flag DESC NULLS FIRST,b.rank ASC",
        "b.score ASC NULLS LAST,b.text DESC NULLS FIRST",
        "b.text ASC NULLS FIRST,b.bytes DESC",
        "b.bytes DESC NULLS LAST",
        "a.rank ASC NULLS LAST,b.flag DESC,a.id DESC",
    ] {
        for limit in [0, 1, 2, 7, 80, 10000] {
            parity(&snapshot, "TRUE", order, limit);
        }
        parity(
            &snapshot,
            "a.id>=50 AND a.id<150 AND (b.rank>=0 OR b.rank IS NULL)",
            order,
            9,
        );
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
}

#[test]
fn equal_sort_keys_keep_left_source_order_across_replacements_and_limits() {
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
        &[
            ("id", DataType::Integer, false),
            ("rank", DataType::Integer, true),
        ],
        0,
    );
    insert(&mut snapshot, 2, vec![Value::Integer(0), Value::Null]);
    for id in [9, 7, 3, 1, 8, 0, 5, 6, 4, 2] {
        insert(
            &mut snapshot,
            1,
            vec![Value::Integer(id), Value::Integer(0)],
        );
    }
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    for limit in [1, 2, 5, 10] {
        let result=query(&snapshot,&format!("SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank DESC NULLS FIRST LIMIT {limit}"),&[]).unwrap();
        assert_eq!(
            result.rows,
            (0..limit)
                .map(|n| vec![Value::Integer(n as i64)])
                .collect::<Vec<_>>()
        );
    }
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Integer(0)),
        })
        .unwrap();
    let current = query(
        &snapshot,
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank LIMIT 2",
        &[],
    )
    .unwrap();
    assert_eq!(
        current.rows,
        [vec![Value::Integer(1)], vec![Value::Integer(2)]]
    );
    assert_eq!(old.page_fingerprint(), digest);
    assert_eq!(
        query(
            &old,
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank LIMIT 1",
            &[]
        )
        .unwrap()
        .rows,
        [vec![Value::Integer(0)]]
    );
}

#[test]
fn retained_bytes_output_limits_and_binding_validation_remain_active() {
    let snapshot = wide();
    let digest = snapshot.page_fingerprint();
    assert!(matches!(
        query(
            &snapshot,
            "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank LIMIT 1500",
            &[]
        ),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    for sql in [
        "SELECT missing FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank LIMIT 0",
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id WHERE b.rank='text' ORDER BY b.rank LIMIT 0",
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY missing LIMIT 0",
        "SELECT a.id FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank LIMIT $1",
    ] {
        assert!(query(&snapshot, sql, &[]).is_err(), "{sql}");
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
}

#[test]
fn complete_predicates_left_points_and_ranges_precede_bounded_sort_selection() {
    let snapshot = typed();
    for filter in [
        "a.id=53",
        "a.id=9999",
        "a.id>=50 AND a.id<100",
        "a.id>=50 AND a.id<50",
        "a.id<10 OR a.id>170",
        "NOT(a.rank=1) AND b.flag IS NOT NULL",
        "FALSE",
    ] {
        parity(
            &snapshot,
            filter,
            "b.text DESC NULLS FIRST,b.score ASC,a.id DESC",
            3,
        );
    }
    let sql = "SELECT a.id,b.rank FROM l AS a JOIN r AS b ON b.id=a.link AND b.rank>=0 WHERE a.id>20 ORDER BY b.rank DESC,a.id LIMIT 4";
    let reference = sql.replace(
        "ON b.id=a.link AND b.rank>=0 WHERE",
        "ON (b.id=a.link AND b.rank>=0) OR FALSE WHERE",
    );
    assert_eq!(
        query(&snapshot, sql, &[]).unwrap(),
        query(&snapshot, &reference, &[]).unwrap()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_nullable_many_to_one_sort_matches_independent_map(
        links in prop::collection::vec(prop::option::of(-2i64..13),0..65),
        lower in 0usize..70,upper in 0usize..70,limit in 0usize..70,
        descending in any::<bool>(),nulls_first in any::<bool>(),
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        table(&mut snapshot,1,"l",&[("id",DataType::Integer,false),("link",DataType::Integer,true)],0);
        table(&mut snapshot,2,"r",&[("rank",DataType::Integer,true),("id",DataType::Integer,false)],1);
        for id in 0..10 {insert(&mut snapshot,2,vec![if id%4==0 {Value::Null}else{Value::Integer(id%3-1)},Value::Integer(id)]);}
        let mut expected=Vec::new();
        for (id,link) in links.iter().enumerate() {
            insert(&mut snapshot,1,vec![Value::Integer(id as i64),link.map_or(Value::Null,Value::Integer)]);
            if id<lower||id>=upper {continue;}
            if let Some(link)=link.filter(|v|(0..10).contains(v)) {
                let rank=if link%4==0 {None}else{Some(link%3-1)};
                if rank.is_none_or(|v|v>=0) {expected.push((id,link,rank));}
            }
        }
        expected.sort_by(|a,b| {
            match (a.2,b.2) {
                (None,None)=>Ordering::Equal,
                (None,_)=>if nulls_first {Ordering::Less}else{Ordering::Greater},
                (_,None)=>if nulls_first {Ordering::Greater}else{Ordering::Less},
                (Some(a),Some(b))=>if descending {b.cmp(&a)}else{a.cmp(&b)},
            }
        });
        expected.truncate(limit);
        let expected=expected.into_iter().map(|(id,link,rank)|vec![Value::Integer(id as i64),Value::Integer(link),rank.map_or(Value::Null,Value::Integer)]).collect::<Vec<_>>();
        let sql=format!("SELECT a.id,b.id,b.rank FROM l AS a JOIN r AS b ON a.link=b.id WHERE a.id>={lower} AND a.id<{upper} AND (b.rank IS NULL OR b.rank>=0) ORDER BY b.rank {} NULLS {} LIMIT {limit}",if descending {"DESC"}else{"ASC"},if nulls_first {"FIRST"}else{"LAST"});
        let reference=sql.replace("ON a.link=b.id WHERE","ON a.link=b.id OR FALSE WHERE");
        let old=snapshot.clone();let digest=old.page_fingerprint();
        let actual=query(&snapshot,&sql,&[]).unwrap();
        prop_assert_eq!(&actual.rows,&expected);
        prop_assert_eq!(actual,query(&snapshot,&reference,&[]).unwrap());
        prop_assert_eq!(snapshot.page_fingerprint(),digest);
        prop_assert_eq!(old.page_fingerprint(),digest);
    }
}

#[test]
fn both_managed_wals_preserve_atomic_staged_sort_reads_and_recovery() {
    for version in [1, 2] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let mut db = Database::create(&path).unwrap();
        if version == 2 {
            db.compact().unwrap();
        }
        assert_eq!(
            emilybase_transactions::recover_image(
                &db.committed_wal().unwrap(),
                Some(db.database_id())
            )
            .unwrap()
            .wal_version,
            version
        );
        execute(&mut db,"CREATE TABLE l(id INT PRIMARY KEY,link INT);CREATE TABLE r(id INT PRIMARY KEY,rank INT);INSERT INTO r VALUES(0,4),(1,2),(2,NULL);INSERT INTO l VALUES(0,0),(1,1),(2,2),(3,1)",&[]).unwrap();
        let old = db.view().unwrap().clone();
        let digest = old.page_fingerprint();
        let transaction = db.last_transaction();
        let wal = db.committed_wal().unwrap();
        let read = "SELECT a.id,b.rank FROM l AS a JOIN r AS b ON a.link=b.id ORDER BY b.rank ASC NULLS LAST LIMIT 2";
        let initial = query(db.view().unwrap(), read, &[]).unwrap();
        assert_eq!(
            initial.rows,
            [
                vec![Value::Integer(1), Value::Integer(2)],
                vec![Value::Integer(3), Value::Integer(2)]
            ]
        );
        let report = execute(
            &mut db,
            &format!("BEGIN;UPDATE r SET rank=-1 WHERE id=0;{read};ROLLBACK"),
            &[],
        )
        .unwrap();
        assert!(!report.committed);
        assert_eq!(
            report.results[1].rows[0],
            [Value::Integer(0), Value::Integer(-1)]
        );
        assert_eq!(db.last_transaction(), transaction);
        assert_eq!(db.committed_wal().unwrap(), wal);
        assert_eq!(query(db.view().unwrap(), read, &[]).unwrap(), initial);
        let invalid = execute(
            &mut db,
            &format!("UPDATE r SET rank=-1 WHERE id=0;{read};SELECT missing FROM r"),
            &[],
        );
        assert!(invalid.is_err());
        assert_eq!(db.committed_wal().unwrap(), wal);
        execute(&mut db, "UPDATE r SET rank=-1 WHERE id=0", &[]).unwrap();
        let final_result = query(db.view().unwrap(), read, &[]).unwrap();
        assert_eq!(
            final_result.rows[0],
            [Value::Integer(0), Value::Integer(-1)]
        );
        assert_eq!(old.page_fingerprint(), digest);
        assert_eq!(query(&old, read, &[]).unwrap(), initial);
        db.checkpoint().unwrap();
        db.compact().unwrap();
        drop(db);
        let reopened = Database::open(path).unwrap();
        assert_eq!(
            query(reopened.view().unwrap(), read, &[]).unwrap(),
            final_result
        );
    }
}
