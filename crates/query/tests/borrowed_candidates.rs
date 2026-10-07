use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use emilybase_query::{ExecutionError, execute, query};
use emilybase_transactions::Database;
fn schema(name: &str) -> Schema {
    Schema {
        name: name.into(),
        columns: (0..64)
            .map(|index| Column {
                name: format!("c{index}"),
                data_type: match index {
                    0 => DataType::Text,
                    1 => DataType::Boolean,
                    2 => DataType::Float,
                    3 => DataType::Bytes,
                    _ => DataType::Integer,
                },
                nullable: index == 62,
            })
            .collect(),
        primary_key: 63,
    }
}
fn row(id: i64) -> Vec<Value> {
    let mut row = (0..64)
        .map(|index| Value::Integer(index + id))
        .collect::<Vec<_>>();
    row[0] = Value::Text(format!("я\0{id}"));
    row[1] = Value::Boolean(id % 2 == 0);
    row[2] = Value::Float(-0.0);
    row[3] = Value::Bytes(vec![0, 255, id as u8]);
    row[62] = if id == 2 {
        Value::Null
    } else {
        Value::Integer(7)
    };
    row[63] = Value::Integer(id);
    row
}
#[test]
fn both_maximum_width_sources_resolve_boundary_fields_and_preserve_old_images() {
    let mut snapshot = Snapshot::empty().unwrap();
    for (table_id, name) in [(1, "a"), (2, "b")] {
        snapshot
            .apply(Event {
                table_id,
                kind: EventKind::Create(schema(name)),
            })
            .unwrap();
        for id in [3, 1, 2] {
            snapshot
                .apply(Event {
                    table_id,
                    kind: EventKind::Insert(row(id)),
                })
                .unwrap();
        }
    }
    let old = snapshot.clone();
    let digest = old.page_fingerprint();
    let sql =
        "SELECT * FROM a AS x JOIN b AS y ON x.c63=y.c63 ORDER BY y.c62 ASC NULLS FIRST LIMIT 3";
    let expected = [2, 1, 3]
        .into_iter()
        .map(|id| [row(id), row(id)].concat())
        .collect::<Vec<_>>();
    let result = query(&snapshot, sql, &[]).unwrap();
    assert_eq!(result.rows, expected);
    assert_eq!(result.columns.len(), 128);
    for output in &result.rows {
        for index in [2, 66] {
            let Value::Float(f) = output[index] else {
                panic!("generated float");
            };
            assert_eq!(f.to_bits(), (-0.0f64).to_bits());
        }
    }
    let selected = "SELECT y.c63 AS id,x.c0 AS left,y.c0 AS right,y.c62 AS rank,y.c0 AS again FROM a AS x JOIN b AS y ON x.c63=y.c63 AND (x.c1 OR y.c1) WHERE y.c62 IS NULL ORDER BY y.c62 NULLS FIRST LIMIT 2";
    let output = query(&snapshot, selected, &[]).unwrap();
    assert_eq!(output.columns, ["id", "left", "right", "rank", "again"]);
    assert_eq!(
        output.rows,
        [vec![
            Value::Integer(2),
            row(2)[0].clone(),
            row(2)[0].clone(),
            Value::Null,
            row(2)[0].clone()
        ]]
    );
    assert_eq!(
        query(
            &snapshot,
            &selected.replace(
                "ON x.c63=y.c63 AND (x.c1 OR y.c1)",
                "ON (x.c63=y.c63 AND (x.c1 OR y.c1)) OR FALSE"
            ),
            &[]
        )
        .unwrap(),
        output
    );
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Integer(2)),
        })
        .unwrap();
    assert!(query(&snapshot, selected, &[]).unwrap().rows.is_empty());
    assert_eq!(query(&old, sql, &[]).unwrap().rows, expected);
    assert_eq!(old.page_fingerprint(), digest);
}
#[test]
fn staged_both_source_views_and_output_refusal_preserve_both_wals_and_restore() {
    for version in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Database::create(&path).unwrap();
        if version == 2 {
            db.compact().unwrap();
        }
        execute(&mut db,"CREATE TABLE a(id INTEGER PRIMARY KEY,rank INTEGER,hidden TEXT); CREATE TABLE b(id INTEGER PRIMARY KEY,rank INTEGER,hidden TEXT)",&[]).unwrap();
        let mut tx = db.begin().unwrap();
        for id in 0..100 {
            for table in ["a", "b"] {
                tx.insert(
                    table,
                    vec![
                        Value::Integer(id),
                        Value::Integer(id),
                        Value::Text("x".repeat(3072)),
                    ],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
        let old = db.view().unwrap().clone();
        let old_hash = old.page_fingerprint();
        let before = db.committed_wal().unwrap();
        let report=execute(&mut db,"BEGIN; UPDATE a SET rank=-1 WHERE id=99; UPDATE b SET rank=-2 WHERE id=99; SELECT a.id,b.rank FROM a JOIN b ON a.id=b.id WHERE a.rank<0 ORDER BY b.rank LIMIT 1; ROLLBACK",&[]).unwrap();
        assert_eq!(
            report.results[2].rows,
            [vec![Value::Integer(99), Value::Integer(-2)]]
        );
        assert_eq!(db.committed_wal().unwrap(), before);
        let fields = vec!["b.hidden"; 32].join(",");
        let failed = format!(
            "UPDATE b SET rank=-3 WHERE id=99; SELECT {fields} FROM a JOIN b ON a.id=b.id ORDER BY b.rank LIMIT 100"
        );
        assert!(matches!(
            execute(&mut db, &failed, &[]),
            Err(ExecutionError::Limit("output bytes"))
        ));
        assert_eq!(db.committed_wal().unwrap(), before);
        assert_eq!(db.view().unwrap().page_fingerprint(), old_hash);
        let sql = "SELECT a.id,b.rank FROM a JOIN b ON a.id=b.id ORDER BY b.rank DESC LIMIT 2";
        let expected = query(&old, sql, &[]).unwrap().rows;
        db.checkpoint().unwrap();
        let backup = dir.path().join("copy.emilybak");
        emilybase_backup::create(&mut db, &backup).unwrap();
        drop(db);
        let restored = dir.path().join("restored");
        emilybase_backup::inspect(&backup).unwrap();
        emilybase_backup::restore(&backup, &restored).unwrap();
        let original = Database::open(&path).unwrap();
        let mut copy = Database::open(&restored).unwrap();
        assert_eq!(
            query(original.view().unwrap(), sql, &[]).unwrap().rows,
            expected
        );
        assert_eq!(
            query(copy.view().unwrap(), sql, &[]).unwrap().rows,
            expected
        );
        execute(&mut copy, "UPDATE b SET rank=-9 WHERE id=99", &[]).unwrap();
        assert_eq!(
            query(original.view().unwrap(), sql, &[]).unwrap().rows,
            expected
        );
        assert_eq!(old.page_fingerprint(), old_hash);
    }
}
