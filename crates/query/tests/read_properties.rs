use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{ExecutionError, execute, query};
use emilybase_transactions::Database;
use proptest::prelude::*;

fn schema() -> Schema {
    Schema {
        name: "t".into(),
        primary_key: 0,
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
            Column {
                name: "active".into(),
                data_type: DataType::Boolean,
                nullable: true,
            },
        ],
    }
}

#[test]
fn detached_query_is_read_only_and_cannot_accept_write_or_control_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("db")).unwrap();
    execute(
        &mut db,
        "CREATE TABLE t(id INT PRIMARY KEY,title TEXT);INSERT INTO t VALUES(1,'original')",
        &[],
    )
    .unwrap();
    let snapshot = db.view().unwrap().clone();
    let pages = snapshot.pages().cloned().collect::<Vec<_>>();
    for sql in [
        "DELETE FROM t",
        "SELECT * FROM t;DELETE FROM t",
        "BEGIN;SELECT * FROM t;COMMIT",
        "DROP TABLE t",
    ] {
        assert!(matches!(
            query(&snapshot, sql, &[]),
            Err(ExecutionError::Control)
        ));
    }
    execute(&mut db, "UPDATE t SET title='later' WHERE id=1", &[]).unwrap();
    drop(db);
    assert_eq!(
        query(&snapshot, "SELECT title FROM t", &[]).unwrap().rows,
        [vec![Value::Text("original".into())]]
    );
    assert_eq!(snapshot.pages().cloned().collect::<Vec<_>>(), pages);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_predicates_projection_null_sort_and_limits_match_independent_rows(
        input in prop::collection::btree_map(-40i64..40,(prop::option::of("[a-zé']{0,10}"),prop::option::of(any::<bool>())),0..50),
        lower in -50i64..50, upper in -50i64..50, limit in 0usize..51, descending in any::<bool>(), nulls_first in any::<bool>()
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        snapshot.apply(Event {table_id:1,kind:EventKind::Create(schema())}).unwrap();
        for (id,(title,active)) in &input {
            snapshot.apply(Event {table_id:1,kind:EventKind::Insert(vec![Value::Integer(*id),title.as_ref().map_or(Value::Null,|s|Value::Text(s.clone())),active.map_or(Value::Null,Value::Boolean)])}).unwrap();
        }
        let before=snapshot.pages().cloned().collect::<Vec<_>>();
        let mut expected=input.iter().filter(|(id,(_,active))|**id>=lower && **id<upper && *active!=Some(false)).collect::<Vec<_>>();
        expected.sort_by(|(aid,(a,_)),(bid,(b,_))|{
            let order=match (a,b) {
                (None,None)=>std::cmp::Ordering::Equal,
                (None,Some(_))=>if nulls_first {std::cmp::Ordering::Less}else{std::cmp::Ordering::Greater},
                (Some(_),None)=>if nulls_first {std::cmp::Ordering::Greater}else{std::cmp::Ordering::Less},
                (Some(a),Some(b))=>if descending {b.cmp(a)}else{a.cmp(b)},
            };
            order.then_with(||bid.cmp(aid))
        });
        let expected=expected.into_iter().take(limit).map(|(id,(title,_))|vec![title.as_ref().map_or(Value::Null,|s|Value::Text(s.clone())),Value::Integer(*id)]).collect::<Vec<_>>();
        let sql=format!("SELECT title AS name,id FROM t WHERE id >= $1 AND id < $2 AND (active OR active IS NULL) ORDER BY title {} NULLS {},id DESC LIMIT $3",if descending {"DESC"}else{"ASC"},if nulls_first {"FIRST"}else{"LAST"});
        let actual=query(&snapshot,&sql,&[Value::Integer(lower),Value::Integer(upper),Value::Integer(limit as i64)]).unwrap();
        prop_assert_eq!(actual.columns,vec!["name","id"]);
        prop_assert_eq!(actual.rows,expected);
        prop_assert_eq!(snapshot.pages().cloned().collect::<Vec<_>>(),before);
    }
    #[test]
    fn generated_self_join_matches_independent_pairs(
        ids in prop::collection::btree_set(-20i64..20,0..25),limit in 0usize..101
    ) {
        let mut snapshot=Snapshot::empty().unwrap();snapshot.apply(Event {table_id:1,kind:EventKind::Create(schema())}).unwrap();
        for id in &ids {snapshot.apply(Event {table_id:1,kind:EventKind::Insert(vec![Value::Integer(*id),Value::Null,Value::Null])}).unwrap();}
        let mut expected=Vec::new();
        for a in &ids {for b in &ids {if a<b {expected.push((*a,*b));}}}
        expected.sort_by(|(a,b),(c,d)|c.cmp(a).then_with(||b.cmp(d)));
        let expected=expected.into_iter().take(limit).map(|(a,b)|vec![Value::Integer(a),Value::Integer(b)]).collect::<Vec<_>>();
        let actual=query(&snapshot,"SELECT a.id,b.id FROM t AS a JOIN t AS b ON a.id < b.id ORDER BY a.id DESC,b.id ASC LIMIT $1",&[Value::Integer(limit as i64)]).unwrap();
        prop_assert_eq!(actual.rows,expected);
    }
}
