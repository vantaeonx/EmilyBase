use emilybase_catalog::{Column, DataType, Key, Row, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{ExecutionError, MAX_OUTPUT_BYTES, execute, query};
use emilybase_transactions::Database;
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
fn insert(snapshot: &mut Snapshot, id: u64, row: Row) {
    snapshot
        .apply(Event {
            table_id: id,
            kind: EventKind::Insert(row),
        })
        .unwrap();
}
fn fields(column: &str, count: usize) -> String {
    std::iter::repeat_n(column, count)
        .collect::<Vec<_>>()
        .join(",")
}
fn wide(count: i64) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    table(
        &mut snapshot,
        1,
        "t",
        &[
            ("id", DataType::Integer, false),
            ("rank", DataType::Integer, false),
            ("payload", DataType::Text, false),
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
    insert(&mut snapshot, 2, vec![Value::Integer(0)]);
    for id in 0..count {
        insert(
            &mut snapshot,
            1,
            vec![
                Value::Integer(id),
                Value::Integer(0),
                Value::Text("я".repeat(1536)),
            ],
        );
    }
    snapshot
}
#[test]
fn every_read_path_preserves_exact_shared_output_boundary_for_repeated_fields() {
    let snapshot = wide(120);
    let old = snapshot.clone();
    let digest = snapshot.page_fingerprint();
    let columns = fields("a.payload", 32);
    let charge = 24 + 32 * (32 + 3072);
    let count = MAX_OUTPUT_BYTES / charge;
    for from in [
        "t AS a",
        "t AS a JOIN r AS b ON a.rank=b.id",
        "t AS a JOIN r AS b ON a.rank=b.id OR FALSE",
    ] {
        for order in ["a.id DESC", "a.rank ASC,a.id DESC"] {
            let sql = format!("SELECT {columns} FROM {from} ORDER BY {order} LIMIT {count}");
            let result = query(&snapshot, &sql, &[]).unwrap();
            assert_eq!(result.rows.len(), count);
            assert_eq!(result.columns, vec!["payload"; 32]);
            assert!(
                result
                    .rows
                    .iter()
                    .all(|row| row == &vec![Value::Text("я".repeat(1536)); 32])
            );
            let sql = format!(
                "SELECT {columns} FROM {from} ORDER BY {order} LIMIT {}",
                count + 1
            );
            assert!(matches!(
                query(&snapshot, &sql, &[]),
                Err(ExecutionError::Limit("output bytes"))
            ));
        }
    }
    assert_eq!(snapshot.page_fingerprint(), digest);
    assert_eq!(old.page_fingerprint(), digest);
}
#[test]
fn maximum_projection_width_preserves_exact_output_and_parser_bounds() {
    let snapshot = wide(64);
    let width = emilybase_catalog::MAX_COLUMNS;
    let columns = fields("payload", width);
    let count = MAX_OUTPUT_BYTES / (24 + width * (32 + 3072));
    for order in ["id", "rank,id"] {
        let result = query(
            &snapshot,
            &format!("SELECT {columns} FROM t ORDER BY {order} LIMIT {count}"),
            &[],
        )
        .unwrap();
        assert_eq!(result.rows.len(), count);
        assert!(result.rows.iter().all(|row| row.len() == width));
        assert!(matches!(
            query(
                &snapshot,
                &format!(
                    "SELECT {columns} FROM t ORDER BY {order} LIMIT {}",
                    count + 1
                ),
                &[]
            ),
            Err(ExecutionError::Limit("output bytes"))
        ));
    }
    let extra = fields("payload", width + 1);
    assert!(matches!(
        query(&snapshot, &format!("SELECT {extra} FROM t LIMIT 0"), &[]),
        Err(ExecutionError::Syntax(emilybase_query::Error::Limit(
            "projection columns"
        )))
    ));
}

#[test]
fn projection_preserves_aliases_repetitions_star_types_and_float_bits() {
    let mut snapshot = Snapshot::empty().unwrap();
    table(
        &mut snapshot,
        1,
        "t",
        &[
            ("text", DataType::Text, true),
            ("flag", DataType::Boolean, true),
            ("id", DataType::Integer, false),
            ("f", DataType::Float, true),
            ("bytes", DataType::Bytes, true),
        ],
        2,
    );
    let row = vec![
        Value::Text("я\0e\u{301}".into()),
        Value::Boolean(true),
        Value::Integer(7),
        Value::Float(-0.0),
        Value::Bytes(vec![0, 255]),
    ];
    insert(&mut snapshot, 1, row.clone());
    insert(
        &mut snapshot,
        1,
        vec![
            Value::Null,
            Value::Null,
            Value::Integer(9),
            Value::Null,
            Value::Null,
        ],
    );
    for order in ["id DESC", "text DESC NULLS LAST"] {
        let result = query(
            &snapshot,
            &format!(
                "SELECT text AS label,bytes AS data,f AS score,f,text FROM t ORDER BY {order}"
            ),
            &[],
        )
        .unwrap();
        assert_eq!(result.columns, ["label", "data", "score", "f", "text"]);
        let selected = result
            .rows
            .iter()
            .find(|row| matches!(row[0], Value::Text(_)))
            .unwrap();
        assert_eq!(
            selected,
            &vec![
                row[0].clone(),
                row[4].clone(),
                row[3].clone(),
                row[3].clone(),
                row[0].clone()
            ]
        );
        for index in [2, 3] {
            let Value::Float(value) = selected[index] else {
                panic!("float fixture")
            };
            assert_eq!(value.to_bits(), (-0.0f64).to_bits());
        }
        let star = query(&snapshot, &format!("SELECT * FROM t ORDER BY {order}"), &[]).unwrap();
        assert!(star.rows.contains(&row));
    }
    let joined = query(
        &snapshot,
        "SELECT * FROM t AS a JOIN t AS b ON a.id=b.id ORDER BY b.text DESC NULLS LAST LIMIT 1",
        &[],
    )
    .unwrap();
    assert_eq!(
        joined.columns,
        [
            "a.text", "a.flag", "a.id", "a.f", "a.bytes", "b.text", "b.flag", "b.id", "b.f",
            "b.bytes"
        ]
    );
    assert_eq!(
        joined.rows,
        [row.iter().chain(&row).cloned().collect::<Row>()]
    );
}
#[test]
fn empty_filtered_and_zero_limit_queries_still_bind_the_complete_projection() {
    let snapshot = wide(0);
    for sql in [
        "SELECT missing FROM t LIMIT 0",
        "SELECT payload,missing FROM t ORDER BY id LIMIT 0",
        "SELECT a.payload,b.missing FROM t AS a JOIN r AS b ON a.rank=b.id ORDER BY b.id LIMIT 0",
        "SELECT payload FROM t WHERE id='wrong' LIMIT 0",
        "SELECT payload FROM t ORDER BY missing LIMIT 0",
    ] {
        assert!(query(&snapshot, sql, &[]).is_err(), "{sql}");
    }
    let result = query(
        &snapshot,
        "SELECT payload,payload FROM t ORDER BY rank LIMIT 0",
        &[],
    )
    .unwrap();
    assert_eq!(result.columns, ["payload", "payload"]);
    assert!(result.rows.is_empty());
    let snapshot = wide(120);
    let columns = fields("payload", emilybase_catalog::MAX_COLUMNS);
    assert!(
        query(
            &snapshot,
            &format!("SELECT {columns} FROM t WHERE FALSE ORDER BY rank"),
            &[]
        )
        .unwrap()
        .rows
        .is_empty()
    );
}
#[test]
fn all_script_results_share_admission_and_failed_output_preserves_both_wals() {
    for compact in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source");
        let restore = directory.path().join("restored");
        let archive = directory.path().join("copy.backup");
        let mut db = Database::create(&path).unwrap();
        if compact {
            db.compact().unwrap();
        }
        execute(
            &mut db,
            "CREATE TABLE t(id INT PRIMARY KEY,payload TEXT)",
            &[],
        )
        .unwrap();
        let values = (0..120)
            .map(|n| format!("({n},$1)"))
            .collect::<Vec<_>>()
            .join(",");
        execute(
            &mut db,
            &format!("INSERT INTO t VALUES {values}"),
            &[Value::Text("я".repeat(1536))],
        )
        .unwrap();
        let old = db.view().unwrap().clone();
        let digest = old.page_fingerprint();
        let wal = db.committed_wal().unwrap();
        let transaction = db.last_transaction();
        let columns = fields("payload", 32);
        for order in ["id", "payload,id"] {
            let sql = format!(
                "UPDATE t SET payload='staged' WHERE id=0;SELECT {columns} FROM t ORDER BY {order} LIMIT 80;SELECT {columns} FROM t ORDER BY {order} LIMIT 30"
            );
            assert!(matches!(
                execute(&mut db, &sql, &[]),
                Err(ExecutionError::Limit("output bytes"))
            ));
            assert_eq!(db.last_transaction(), transaction);
            assert_eq!(db.committed_wal().unwrap(), wal);
            assert_eq!(db.view().unwrap().page_fingerprint(), digest);
        }
        assert_eq!(
            emilybase_transactions::recover_image(&wal, Some(db.database_id()))
                .unwrap()
                .wal_version,
            if compact { 2 } else { 1 }
        );
        let report=execute(&mut db,&format!("BEGIN;UPDATE t SET payload='temporary' WHERE id=0;SELECT {columns} FROM t ORDER BY payload,id LIMIT 2;ROLLBACK"),&[]).unwrap();
        assert!(!report.committed);
        assert_eq!(db.committed_wal().unwrap(), wal);
        db.checkpoint().unwrap();
        drop(db);
        let mut db = Database::open(&path).unwrap();
        assert_eq!(db.last_transaction(), transaction);
        emilybase_backup::create(&mut db, &archive).unwrap();
        emilybase_backup::inspect(&archive).unwrap();
        emilybase_backup::restore(&archive, &restore).unwrap();
        let mut copy = Database::open(restore).unwrap();
        assert_eq!(copy.last_transaction(), transaction);
        assert!(matches!(
            query(
                copy.view().unwrap(),
                &format!("SELECT {columns} FROM t ORDER BY payload,id LIMIT 120"),
                &[]
            ),
            Err(ExecutionError::Limit("output bytes"))
        ));
        execute(
            &mut copy,
            "UPDATE t SET payload='independent' WHERE id=0",
            &[],
        )
        .unwrap();
        assert_eq!(
            db.view()
                .unwrap()
                .get("t", &Key::Integer(0))
                .unwrap()
                .unwrap()[1],
            Value::Text("я".repeat(1536))
        );
        assert_eq!(old.page_fingerprint(), digest);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_repeated_nullable_columns_match_an_independent_projection(
        text in "[a-zяλ]{0,32}", bytes in prop::collection::vec(any::<u8>(),0..32),
        rows in 0usize..35,repeats in 1usize..45,descending in any::<bool>(),
        nulls in prop::collection::vec(any::<bool>(),35),
    ) {
        let mut snapshot=Snapshot::empty().unwrap();
        table(&mut snapshot,1,"t",&[("id",DataType::Integer,false),("text",DataType::Text,true),("bytes",DataType::Bytes,true)],0);
        let mut model=Vec::new();
        for (id,null) in nulls.iter().take(rows).enumerate() {
            let row=vec![Value::Integer(id as i64),if *null{Value::Null}else{Value::Text(text.clone())},if *null{Value::Null}else{Value::Bytes(bytes.clone())}];
            insert(&mut snapshot,1,row.clone());model.push(row);
        }
        if descending{model.reverse();}
        let columns=(0..repeats).map(|n|if n%2==0{"text"}else{"bytes"}).collect::<Vec<_>>().join(",");
        let expected=model.iter().map(|row|(0..repeats).map(|n|row[if n%2==0{1}else{2}].clone()).collect::<Row>()).collect::<Vec<_>>();
        let sql=format!("SELECT {columns} FROM t ORDER BY id {}",if descending{"DESC"}else{"ASC"});
        let old=snapshot.clone();let digest=old.page_fingerprint();
        prop_assert_eq!(query(&snapshot,&sql,&[]).unwrap().rows,expected);
        prop_assert_eq!(snapshot.page_fingerprint(),digest);prop_assert_eq!(old.page_fingerprint(),digest);
    }
}
