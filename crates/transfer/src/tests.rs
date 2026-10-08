use super::*;
use emilybase_catalog::{Column, DataType, Value};
use proptest::prelude::*;
use serde_json::{Value as Json, json};
use std::sync::Mutex;
static CASES: Mutex<()> = Mutex::new(());
fn schema() -> Schema {
    Schema {
        name: "synthetic_table".into(),
        primary_key: 0,
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "text".into(),
                data_type: DataType::Text,
                nullable: false,
            },
            Column {
                name: "number".into(),
                data_type: DataType::Float,
                nullable: false,
            },
            Column {
                name: "raw".into(),
                data_type: DataType::Bytes,
                nullable: true,
            },
            Column {
                name: "flag".into(),
                data_type: DataType::Boolean,
                nullable: false,
            },
        ],
    }
}
fn row(id: i64, bits: u64) -> Row {
    vec![
        Value::Integer(id),
        Value::Text("synthetic-Привет-界\0'; DROP TABLE x; --".into()),
        Value::Float(f64::from_bits(bits)),
        Value::Bytes(vec![0, 255, 1]),
        Value::Boolean(true),
    ]
}
fn source(path: &std::path::Path, rows: Vec<Row>) -> Database {
    let mut db = Database::create(path).unwrap();
    let mut tx = db.begin().unwrap();
    tx.create_table(schema()).unwrap();
    for row in rows {
        tx.insert("synthetic_table", row).unwrap();
    }
    tx.commit().unwrap();
    db
}
fn document() -> Json {
    json!({"format":MAGIC,"version":1,"schema":schema(),"rows":[[
        {"type":"integer","value":1},{"type":"text","value":"synthetic-private-content"},
        {"type":"float_bits","value":"8000000000000000"},{"type":"null"},{"type":"boolean","value":false}
    ]]})
}
fn bytes(value: &Json) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
#[test]
fn types_and_floating_bits_roundtrip_on_both_wal_versions_without_source_writes() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for compact in [false, true] {
        let mut db = source(
            &dir.path().join(format!("source-{compact}")),
            vec![
                row(i64::MAX, 0x7fefffffffffffff),
                row(i64::MIN, 0x8000000000000000),
                row(0, 1),
            ],
        );
        if compact {
            db.compact().unwrap();
        }
        let before = db.committed_wal().unwrap();
        let encoded = export_table(db.view().unwrap(), "synthetic_table").unwrap();
        assert_eq!(db.committed_wal().unwrap(), before);
        let verified = decode_table(&encoded).unwrap();
        assert_eq!(verified.report.rows, 3);
        assert_eq!(verified.report.columns, 5);
        assert!(!format!("{verified:?}").contains("Привет"));
        let target = dir.path().join(format!("target-{compact}"));
        let mut copy = Database::create(&target).unwrap();
        if compact {
            copy.compact().unwrap();
        }
        assert_eq!(import_table(&mut copy, verified).unwrap(), 2);
        assert_eq!(
            export_table(copy.view().unwrap(), "synthetic_table").unwrap(),
            encoded
        );
        drop(copy);
        let mut copy = Database::open(target).unwrap();
        let before = copy.committed_wal().unwrap();
        assert!(matches!(
            import_table(&mut copy, decode_table(&encoded).unwrap()),
            Err(Error::Existing)
        ));
        assert_eq!(copy.committed_wal().unwrap(), before);
    }
}
#[test]
fn strict_document_shape_order_and_types_refuse_without_echo() {
    let _guard = CASES.lock().unwrap();
    let original = document();
    let mut cases = vec![
        b"".to_vec(),
        b"{}".to_vec(),
        b"[]".to_vec(),
        b"{\"format\":\"emilybase-table\",\"format\":\"emilybase-table\"}".to_vec(),
    ];
    for (field, value) in [
        ("format", json!("other")),
        ("version", json!(2)),
        ("extra", json!("synthetic-private-content")),
        ("rows", json!([[], []])),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        cases.push(bytes(&changed));
    }
    for value in [
        json!({"type":"float","value":0}),
        json!({"type":"float_bits","value":"7ff0000000000000"}),
        json!({"type":"float_bits","value":"7ff8000000000000"}),
        json!({"type":"float_bits","value":"ABCDEF0000000000"}),
        json!({"type":"float_bits","value":"0"}),
        json!({"type":"float_bits","value":"8000000000000000","extra":1}),
    ] {
        let mut changed = original.clone();
        changed["rows"][0][2] = value;
        cases.push(bytes(&changed));
    }
    let mut duplicate = original.clone();
    let item = duplicate["rows"][0].clone();
    duplicate["rows"].as_array_mut().unwrap().push(item);
    cases.push(bytes(&duplicate));
    let mut descending = original.clone();
    let mut item = descending["rows"][0].clone();
    item[0]["value"] = json!(0);
    descending["rows"].as_array_mut().unwrap().push(item);
    cases.push(bytes(&descending));
    for bad in cases {
        let error = decode_table(&bad).unwrap_err();
        assert!(!error.to_string().contains("synthetic-private-content"));
    }
    assert_eq!(decode_table(&bytes(&original)).unwrap().report.rows, 1);
}
#[test]
fn bounded_elements_values_encoding_and_input_bytes_refuse() {
    let _guard = CASES.lock().unwrap();
    let mut value = document();
    let item = value["rows"][0].clone();
    value["rows"] = json!(vec![item; MAX_TRANSFER_ROWS + 1]);
    assert!(decode_table(&bytes(&value)).is_err());
    let mut value = document();
    let column = value["schema"]["columns"][0].clone();
    value["schema"]["columns"] = json!(vec![column; 65]);
    assert!(decode_table(&bytes(&value)).is_err());
    let mut value = document();
    value["rows"][0][1]["value"] = json!("x".repeat(3073));
    assert!(decode_table(&bytes(&value)).is_err());
    let mut value = document();
    value["rows"][0][3] = json!({"type":"bytes","value":vec![255;3073]});
    assert!(decode_table(&bytes(&value)).is_err());
    let mut value = document();
    value["schema"]["name"] = json!("x".repeat(64));
    assert!(decode_table(&bytes(&value)).is_err());
    let mut value = document();
    let item = value["rows"][0][0].clone();
    value["rows"][0] = json!(vec![item; 65]);
    assert!(decode_table(&bytes(&value)).is_err());
    let mut value = document();
    value["rows"][0][1]["value"] = json!("x".repeat(3072));
    value["rows"][0][3] = json!({"type":"bytes","value":vec![0;3072]});
    assert!(matches!(
        decode_table(&bytes(&value)),
        Err(Error::Catalog(_))
    ));
    assert!(matches!(
        decode_table(&vec![b' '; MAX_TRANSFER_BYTES + 1]),
        Err(Error::Limit)
    ));
    struct Infinite(usize);
    impl Read for Infinite {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            out.fill(b' ');
            self.0 += out.len();
            Ok(out.len())
        }
    }
    let mut infinite = Infinite(0);
    assert!(matches!(read_table(&mut infinite), Err(Error::Limit)));
    assert_eq!(infinite.0, MAX_TRANSFER_BYTES + 1);
}
#[test]
fn row_capacity_empty_tables_and_full_atomic_transaction_boundary() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut db = source(
        &dir.path().join("full"),
        (0..MAX_TRANSFER_ROWS).map(|i| row(i as i64, 0)).collect(),
    );
    let encoded = export_table(db.view().unwrap(), "synthetic_table").unwrap();
    let mut target = Database::create(dir.path().join("copy")).unwrap();
    import_table(&mut target, decode_table(&encoded).unwrap()).unwrap();
    assert_eq!(target.view().unwrap().row_count(), MAX_TRANSFER_ROWS);
    let mut tx = db.begin().unwrap();
    tx.insert("synthetic_table", row(256, 0)).unwrap();
    tx.commit().unwrap();
    let before = db.committed_wal().unwrap();
    assert!(matches!(
        export_table(db.view().unwrap(), "synthetic_table"),
        Err(Error::Limit)
    ));
    assert_eq!(db.committed_wal().unwrap(), before);
    let empty = source(&dir.path().join("empty"), vec![]);
    let data = export_table(empty.view().unwrap(), "synthetic_table").unwrap();
    let mut copy = Database::create(dir.path().join("empty-copy")).unwrap();
    import_table(&mut copy, decode_table(&data).unwrap()).unwrap();
    assert_eq!(copy.view().unwrap().table_count(), 1);
    assert_eq!(copy.view().unwrap().row_count(), 0);
}
#[test]
fn text_primary_keys_include_maximum_length_utf8_order_and_do_not_execute_payload() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::create(dir.path().join("source")).unwrap();
    let schema = Schema {
        name: "t".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Text,
            nullable: false,
        }],
        primary_key: 0,
    };
    let mut tx = db.begin().unwrap();
    tx.create_table(schema).unwrap();
    for key in [
        "界".into(),
        "'; DROP TABLE t; --".into(),
        "x".repeat(3072),
        "".into(),
    ] {
        tx.insert("t", vec![Value::Text(key)]).unwrap();
    }
    tx.commit().unwrap();
    let data = export_table(db.view().unwrap(), "t").unwrap();
    let mut copy = Database::create(dir.path().join("copy")).unwrap();
    import_table(&mut copy, decode_table(&data).unwrap()).unwrap();
    assert_eq!(export_table(copy.view().unwrap(), "t").unwrap(), data);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_finite_bits_and_integer_order_preserve_exact_values(bits in prop::collection::vec(any::<u64>(),0..24)) {
        let _guard=CASES.lock().unwrap();let dir=tempfile::tempdir().unwrap();
        let expected:Vec<_>=bits.into_iter().filter(|v|f64::from_bits(*v).is_finite()).collect();
        let rows:Vec<_>=expected.iter().enumerate().map(|(i,b)|row(i as i64,*b)).rev().collect();
        let db=source(&dir.path().join("source"),rows);
        let data=export_table(db.view().unwrap(),"synthetic_table").unwrap();
        let mut copy=Database::create(dir.path().join("copy")).unwrap();import_table(&mut copy,decode_table(&data).unwrap()).unwrap();
        let actual:Vec<_>=copy.view().unwrap().scan("synthetic_table",MAX_TRANSFER_ROWS).unwrap().iter().map(|r|match r[2]{Value::Float(v)=>v.to_bits(),_=>panic!("wrong generated value type")}).collect();
        prop_assert_eq!(actual,expected);
        prop_assert_eq!(export_table(copy.view().unwrap(),"synthetic_table").unwrap(),data);
    }
}

#[test]
#[ignore = "Native crash helper, invoked only by the deadline-controlled parent case"]
fn native_import_helper() {
    use std::io::Write;
    let table = read_table(std::io::stdin().lock()).unwrap();
    let mut db = Database::open(std::env::var_os("EMILYBASE_TRANSFER_TEST_ROOT").unwrap()).unwrap();
    let before = std::env::var_os("EMILYBASE_TRANSFER_TEST_BEFORE").is_some();
    let pause = || {
        println!("staged");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    };
    if before {
        let _ = import_after_stage(&mut db, table, pause);
    } else {
        let transaction = import_table(&mut db, table).unwrap();
        assert_eq!(transaction, 2);
        println!("acknowledged");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
#[test]
fn native_kills_before_commit_and_after_ack_recover_exact_all_or_nothing() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::time::Duration;
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = source(
        &dir.path().join("source"),
        vec![row(2, 1), row(1, 0x8000000000000000)],
    );
    let encoded = export_table(db.view().unwrap(), "synthetic_table").unwrap();
    drop(db);
    for compact in [false, true] {
        for before in [false, true] {
            let path = dir.path().join(format!("target-{compact}-{before}"));
            let mut target = Database::create(&path).unwrap();
            if compact {
                target.compact().unwrap();
            }
            let original = target.committed_wal().unwrap();
            drop(target);
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "tests::native_import_helper",
                    "--nocapture",
                ])
                .env("EMILYBASE_TRANSFER_TEST_ROOT", &path)
                .env_remove("EMILYBASE_TRANSFER_TEST_BEFORE")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if before {
                command.env("EMILYBASE_TRANSFER_TEST_BEFORE", "1");
            }
            let mut child = command.spawn().unwrap();
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(&encoded).unwrap();
            drop(stdin);
            let stdout = child.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let ready = if before { "staged" } else { "acknowledged" };
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap() == ready {
                        let _ = send.send(());
                        break;
                    }
                }
            });
            let result = receive.recv_timeout(Duration::from_secs(10));
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            reader.join().unwrap();
            result.expect("native import did not reach expected phase");
            assert!(!output.status.success());
            assert!(output.stderr.is_empty());
            let mut target = Database::open(&path).unwrap();
            if before {
                assert_eq!(target.view().unwrap().table_count(), 0);
                assert_eq!(target.committed_wal().unwrap(), original);
            } else {
                assert_eq!(target.last_transaction(), 2);
                assert_eq!(
                    export_table(target.view().unwrap(), "synthetic_table").unwrap(),
                    encoded
                );
            }
        }
    }
}

#[test]
fn smaller_export_byte_caps_refuse_before_partial_output_and_keep_wal_exact() {
    let _guard = CASES.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut db = source(&dir.path().join("source"), vec![row(1, 1)]);
    let original = export_table(db.view().unwrap(), "synthetic_table").unwrap();
    let before = db.committed_wal().unwrap();
    for cap in [0, 1, original.len() - 1, MAX_TRANSFER_BYTES + 1] {
        assert!(matches!(
            export_table_bounded(db.view().unwrap(), "synthetic_table", cap),
            Err(Error::Limit)
        ));
    }
    assert_eq!(
        export_table_bounded(db.view().unwrap(), "synthetic_table", original.len()).unwrap(),
        original
    );
    assert_eq!(db.committed_wal().unwrap(), before);
}
