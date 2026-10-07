use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_query::{execute, explain, query};
use emilybase_transactions::Database;
use proptest::prelude::*;

fn initialized(path: &std::path::Path, keys: &[String]) -> Database {
    let mut database = Database::create(path).unwrap();
    let mut transaction = database.begin().unwrap();
    transaction
        .create_table(Schema {
            name: "t".into(),
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
                Column {
                    name: "n".into(),
                    data_type: DataType::Integer,
                    nullable: true,
                },
            ],
            primary_key: 0,
        })
        .unwrap();
    for (index, key) in keys.iter().enumerate() {
        transaction
            .insert(
                "t",
                vec![
                    Value::Text(key.clone()),
                    if index % 3 == 0 {
                        Value::Null
                    } else {
                        Value::Integer(index as i64)
                    },
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    database
}
fn selected(database: &Database, predicate: &str, parameters: &[Value]) -> Vec<String> {
    query(
        database.view().unwrap(),
        &format!("SELECT id FROM t WHERE {predicate} ORDER BY id"),
        parameters,
    )
    .unwrap()
    .rows
    .into_iter()
    .map(|row| match row.into_iter().next().unwrap() {
        Value::Text(key) => key,
        _ => panic!("wrong type"),
    })
    .collect()
}

#[test]
fn short_text_constraints_plan_primary_ranges_without_losing_long_prefix_keys() {
    let dir = tempfile::tempdir().unwrap();
    let keys = ["", "\0", "a", "a\0", "a\0x", "ab", "b", "界", "界a", "😀"]
        .map(str::to_owned)
        .into_iter()
        .chain([format!("a{}", "z".repeat(3071))])
        .collect::<Vec<_>>();
    let database = initialized(&dir.path().join("db"), &keys);
    let parameters = [Value::Text("a".into()), Value::Text("b".into())];
    let source = "SELECT id FROM t WHERE id >= $1 AND id < $2 ORDER BY id";
    assert_eq!(
        explain(database.view().unwrap(), source, &parameters)
            .unwrap()
            .access,
        "primary_range"
    );
    let mut expected = keys
        .into_iter()
        .filter(|key| key.as_str() >= "a" && key.as_str() < "b")
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(
        selected(&database, "id >= $1 AND id < $2", &parameters),
        expected
    );
}

#[test]
fn text_successors_reversed_operands_and_key_size_edges_preserve_exact_comparisons() {
    let dir = tempfile::tempdir().unwrap();
    let keys = [
        String::new(),
        "\0".into(),
        "a".into(),
        "a\0".into(),
        "a\0x".into(),
        "ab".into(),
        "b".into(),
        "界".into(),
        "😀".into(),
        "z".repeat(255),
        "z".repeat(256),
        "z".repeat(257),
        "z".repeat(3072),
    ];
    let database = initialized(&dir.path().join("db"), &keys);
    for bound in [
        "",
        "a",
        "a\0",
        "界",
        "😀",
        &"z".repeat(255),
        &"z".repeat(256),
        &"z".repeat(3072),
    ] {
        for (op, reverse) in [(">", "<"), (">=", "<="), ("<", ">"), ("<=", ">=")] {
            let mut expected = keys
                .iter()
                .filter(|key| match op {
                    ">" => key.as_str() > bound,
                    ">=" => key.as_str() >= bound,
                    "<" => key.as_str() < bound,
                    _ => key.as_str() <= bound,
                })
                .cloned()
                .collect::<Vec<_>>();
            expected.sort();
            let parameter = [Value::Text(bound.into())];
            for predicate in [format!("id {op} $1"), format!("$1 {reverse} id")] {
                assert_eq!(selected(&database, &predicate, &parameter), expected);
                let access = explain(
                    database.view().unwrap(),
                    &format!("SELECT id FROM t WHERE {predicate}"),
                    &parameter,
                )
                .unwrap()
                .access;
                let expected_access =
                    if bound.len() > 256 || bound.len() == 256 && matches!(op, ">" | "<=") {
                        "scan"
                    } else {
                        "primary_range"
                    };
                assert_eq!(access, expected_access);
            }
        }
    }
}

#[test]
fn text_ranges_keep_full_filter_type_null_alias_order_and_limit_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let mut database = initialized(
        &dir.path().join("db"),
        &["a".into(), "b".into(), "c".into(), "d".into()],
    );
    assert_eq!(
        query(
            database.view().unwrap(),
            "SELECT x.id FROM t AS x WHERE x.id >= 'a' AND x.n > 0 ORDER BY x.id DESC LIMIT 1",
            &[]
        )
        .unwrap()
        .rows,
        vec![vec![Value::Text("c".into())]]
    );
    for predicate in [
        "id > 'a' OR id = 'a'",
        "NOT (id < 'b')",
        "id > NULL",
        "n >= 0",
    ] {
        assert_eq!(
            explain(
                database.view().unwrap(),
                &format!("SELECT id FROM t WHERE {predicate}"),
                &[]
            )
            .unwrap()
            .access,
            "scan"
        );
    }
    assert!(selected(&database, "id > NULL", &[]).is_empty());
    assert!(selected(&database, "id >= 'z' AND id < 'a'", &[]).is_empty());
    for source in [
        "SELECT missing FROM t WHERE id > 'z' LIMIT 0",
        "SELECT id FROM t WHERE id > 7 LIMIT 0",
        "SELECT id FROM t WHERE id > $1 LIMIT 0",
        "SELECT id FROM t WHERE id < '' AND missing=1",
    ] {
        assert!(query(database.view().unwrap(), source, &[]).is_err());
    }
    execute(
        &mut database,
        "CREATE TABLE other(n INT,id TEXT PRIMARY KEY); INSERT INTO other VALUES (9,'b')",
        &[],
    )
    .unwrap();
    assert_eq!(
        query(
            database.view().unwrap(),
            "SELECT x.n FROM other AS x WHERE x.id > 'a' AND x.id <= 'b'",
            &[]
        )
        .unwrap()
        .rows,
        vec![vec![Value::Integer(9)]]
    );
    assert_eq!(
        explain(
            database.view().unwrap(),
            "SELECT a.id FROM t AS a JOIN t AS b ON a.id=b.id WHERE a.id>'a'",
            &[]
        )
        .unwrap()
        .access,
        "primary_join"
    );
}

#[test]
fn text_range_writes_are_atomic_and_survive_cache_reopen_compaction_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let long = format!("a{}", "x".repeat(3071));
    let mut database = initialized(&path, &["a".into(), "b".into(), long.clone()]);
    database.save_primary_index_cache("t").unwrap();
    let wal = database.committed_wal().unwrap();
    execute(
        &mut database,
        "BEGIN; UPDATE t SET n=9 WHERE id>='a' AND id<'b'; DELETE FROM t WHERE id>='b'; ROLLBACK",
        &[],
    )
    .unwrap();
    assert_eq!(database.committed_wal().unwrap(), wal);
    assert!(
        execute(
            &mut database,
            "UPDATE t SET n=9 WHERE id>='a' AND id<'b'; INSERT INTO t VALUES ('a',1)",
            &[]
        )
        .is_err()
    );
    assert_eq!(database.committed_wal().unwrap(), wal);
    let result = execute(
        &mut database,
        "UPDATE t SET n=9 WHERE id>='a' AND id<'b'; DELETE FROM t WHERE id>'a' AND id<'b'",
        &[],
    )
    .unwrap();
    assert_eq!(result.results[0].affected, 2);
    assert_eq!(result.results[1].affected, 1);
    assert_eq!(selected(&database, "id>='a'", &[]), vec!["a", "b"]);
    for compacted in [false, true] {
        if compacted {
            database.compact().unwrap();
        }
        let archive = dir.path().join(format!("synthetic-{compacted}.backup"));
        emilybase_backup::create(&mut database, &archive).unwrap();
        let target = dir.path().join(format!("copy-{compacted}"));
        emilybase_backup::restore(&archive, &target).unwrap();
        let restored = Database::open(target).unwrap();
        assert_eq!(selected(&restored, "id>='a'", &[]), vec!["a", "b"]);
        assert_eq!(
            query(
                restored.view().unwrap(),
                "SELECT n FROM t WHERE id <= 'a'",
                &[]
            )
            .unwrap()
            .rows,
            vec![vec![Value::Integer(9)]]
        );
    }
    drop(database);
    let database = Database::open(path).unwrap();
    assert_eq!(database.primary_cache_startup().unwrap().rejected, 1);
    assert_eq!(selected(&database, "id > 'a'", &[]), vec!["b"]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn generated_text_comparisons_match_an_independent_nullable_row_model(
        input in proptest::collection::btree_set("[a-z界😀\\x00]{0,8}",0..30),
        lower in "[a-z界😀\\x00]{0,8}", upper in "[a-z界😀\\x00]{0,8}",
        strict_lower in any::<bool>(), inclusive_upper in any::<bool>(), limit in 0usize..40
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut keys = input.into_iter().collect::<Vec<_>>();
        keys.push(format!("a{}", "界".repeat(1023)));
        keys.sort(); keys.dedup();
        let database = initialized(&dir.path().join("db"), &keys);
        let lo = if strict_lower { ">" } else { ">=" };
        let hi = if inclusive_upper { "<=" } else { "<" };
        let source = format!("SELECT id FROM t WHERE id{lo}$1 AND id{hi}$2 AND n>=0 ORDER BY id DESC LIMIT $3");
        let parameters = [Value::Text(lower.clone()), Value::Text(upper.clone()), Value::Integer(limit as i64)];
        let expected = keys.iter().enumerate().rev().filter(|(index,key)| index % 3 != 0 && if strict_lower { **key > lower } else { **key >= lower }).filter(|(_,key)| if inclusive_upper { **key <= upper } else { **key < upper }).take(limit).map(|(_,key)|vec![Value::Text(key.clone())]).collect::<Vec<_>>();
        prop_assert_eq!(query(database.view().unwrap(), &source, &parameters).unwrap().rows, expected);
        prop_assert_eq!(explain(database.view().unwrap(), &source, &parameters).unwrap().access, "primary_range");
    }
}
