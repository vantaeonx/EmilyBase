use emilybase_catalog::{DataType, Value};
use emilybase_query::{ast::*, *};
use proptest::prelude::*;

#[test]
fn x_columns_and_qualifiers_are_distinguished_from_hex_literal_prefixes() {
    for name in ["x", "X", "xray"] {
        let sql = format!("SELECT {name}.id FROM t AS {name} WHERE {name}.id>=1");
        assert!(parse(&sql).is_ok());
        let sql = format!("SELECT {name} FROM t WHERE {name}=1");
        let parsed = parse(&sql).unwrap();
        let Statement::Select(select) = &parsed[0] else {
            panic!("expected SELECT")
        };
        assert!(matches!(
            select.filter,
            Some(Expr::Compare(Operand::Column(_), Compare::Eq, _))
        ));
    }
    let parsed = parse("SELECT payload FROM t WHERE payload=X'00ff'").unwrap();
    let Statement::Select(select) = &parsed[0] else {
        panic!("expected SELECT")
    };
    assert!(
        matches!(&select.filter,Some(Expr::Compare(_,Compare::Eq,Operand::Scalar(Scalar::Literal(Value::Bytes(bytes))))) if bytes==&[0,255])
    );
    for hex in ["X'0'", "X'gg'", "X'00", "X'"] {
        assert!(parse(&format!("SELECT payload FROM t WHERE payload={hex}")).is_err());
    }
}

#[test]
fn complete_synthetic_script_has_typed_statements_and_bound_parameters() {
    let script = "BEGIN TRANSACTION; CREATE TABLE items (id BIGINT PRIMARY KEY, title TEXT, active BOOL NOT NULL, price DOUBLE, payload BYTES); INSERT INTO items (id,title) VALUES (1,'été'),($1,$2); SELECT title AS label FROM items WHERE active = TRUE ORDER BY id DESC NULLS FIRST LIMIT $3; UPDATE items SET title='new',price=1.5 WHERE id=1; DELETE FROM items WHERE title IS NULL; DROP TABLE items; COMMIT;";
    let statements = parse(script).unwrap();
    assert_eq!(statements.len(), 8);
    let Statement::Create(schema) = &statements[1] else {
        panic!()
    };
    assert_eq!(schema.columns[0].data_type, DataType::Integer);
    assert!(!schema.columns[0].nullable);
    assert!(schema.columns[1].nullable);
    let Statement::Insert { rows, .. } = &statements[2] else {
        panic!()
    };
    assert_eq!(rows[1], [Scalar::Parameter(1), Scalar::Parameter(2)]);
    let Statement::Select(select) = &statements[3] else {
        panic!()
    };
    assert_eq!(
        select.columns.as_ref().unwrap()[0].alias.as_deref(),
        Some("label")
    );
    assert!(select.order[0].descending && select.order[0].nulls_first);
    assert_eq!(select.limit, Some(Scalar::Parameter(3)));
    assert_eq!(statements[7], Statement::Commit);
}

#[test]
fn quoted_values_comments_and_case_preserve_identifier_and_literal_contents() {
    let parsed = parse("/* prefix */ iNsErT INTO \"Items\" VALUES (-9223372036854775808,'l''été; -- /* */', X'00Ff', FALSE, NULL, 1e-2); -- tail").unwrap();
    let Statement::Insert { table, rows, .. } = &parsed[0] else {
        panic!()
    };
    assert_eq!(table, "Items");
    assert_eq!(
        rows[0],
        [
            Scalar::Literal(Value::Integer(i64::MIN)),
            Scalar::Literal(Value::Text("l'été; -- /* */".into())),
            Scalar::Literal(Value::Bytes(vec![0, 255])),
            Scalar::Literal(Value::Boolean(false)),
            Scalar::Literal(Value::Null),
            Scalar::Literal(Value::Float(0.01))
        ]
    );
    assert!(parse("CREATE TABLE T (value TEXT, id INTEGER, PRIMARY KEY (id))").is_ok());
    assert!(parse("SELECT * FROM T LIMIT 0").is_ok());
    assert!(parse("ROLLBACK;").is_ok());
}

#[test]
fn boolean_precedence_and_qualified_join_have_an_original_ast() {
    let parsed = parse("SELECT a.id,b.title FROM items AS a INNER JOIN labels AS b ON a.id=b.owner WHERE NOT a.active OR a.id>=2 AND b.title IS NOT NULL ORDER BY b.title ASC,a.id DESC LIMIT 7").unwrap();
    let Statement::Select(select) = &parsed[0] else {
        panic!()
    };
    let Expr::Or(left, right) = select.filter.as_ref().unwrap() else {
        panic!()
    };
    assert!(matches!(**left, Expr::Not(_)));
    assert!(matches!(**right, Expr::And(_, _)));
    assert!(matches!(
        select.join.as_ref().unwrap().1,
        Expr::Compare(_, Compare::Eq, _)
    ));
    assert_eq!(select.order.len(), 2);
    for op in ["=", "<>", "!=", "<", "<=", ">", ">="] {
        assert!(parse(&format!("SELECT * FROM t WHERE id {op} $1")).is_ok());
    }
}

#[test]
fn unsupported_or_malformed_input_is_rejected_without_echoing_literals() {
    for sql in [
        "",
        "-- empty",
        "SELECT *",
        "SELECT * FROM t SELECT * FROM t",
        "SELECT COUNT(*) FROM t",
        "SELECT * FROM t LEFT JOIN x ON t.id=x.id",
        "CREATE INDEX i ON t(id)",
        "UPDATE t SET id=id+1",
        "INSERT INTO t VALUES (1,)",
        "INSERT INTO t VALUES ()",
        "SELECT * FROM t WHERE id ! 1",
        "SELECT * FROM t; ;",
        "INSERT INTO t VALUES ('secret-never-echo)",
        "/* unterminated",
        "CREATE TABLE t(id INT)",
        "CREATE TABLE t(id FLOAT PRIMARY KEY)",
        "CREATE TABLE t(id INT PRIMARY KEY, id TEXT)",
        "CREATE TABLE t(id INT PRIMARY KEY, x TEXT PRIMARY KEY)",
        "CREATE TABLE t(id INT, PRIMARY KEY(missing))",
        "CREATE TABLE t(\"é\" INT PRIMARY KEY)",
        "SELECT * FROM \"\"",
        "SELECT * FROM t LIMIT -1",
        "SELECT * FROM t LIMIT 10001",
        "SELECT * FROM t LIMIT 1.0",
        "SELECT * FROM t LIMIT NULL",
        "INSERT INTO t VALUES (9223372036854775808)",
        "INSERT INTO t VALUES (-9223372036854775809)",
        "INSERT INTO t VALUES (1e999)",
        "INSERT INTO t VALUES (1e+)",
        "INSERT INTO t VALUES (X'0')",
        "INSERT INTO t VALUES (X'GG')",
        "SELECT * FROM t WHERE id=$0",
        "SELECT * FROM t WHERE id=$257",
        "SELECT * FROM t WHERE id=$999999999999999999999999999",
        "SELECT * FROM t WHERE id=$",
        "SELECT * FROM t\0",
    ] {
        let error = parse(sql).expect_err(sql).to_string();
        assert!(!error.contains("secret-never-echo"));
    }
}

#[test]
fn all_boundaries_are_enforced_before_unbounded_work() {
    assert!(
        parse(&format!(
            "SELECT * FROM t{}",
            " ".repeat(MAX_SQL_BYTES - 15)
        ))
        .is_ok()
    );
    assert_eq!(
        parse(&" ".repeat(MAX_SQL_BYTES + 1)),
        Err(Error::Limit("input bytes"))
    );
    assert_eq!(
        parse(&"BEGIN;".repeat(MAX_STATEMENTS + 1)),
        Err(Error::Limit("statements"))
    );
    assert!(parse(&"BEGIN;".repeat(MAX_STATEMENTS)).is_ok());
    assert!(parse(&format!("SELECT {} FROM t", "id,".repeat(63) + "id")).is_ok());
    assert_eq!(
        parse(&format!("SELECT {} FROM t", "id,".repeat(64) + "id")),
        Err(Error::Limit("projection columns"))
    );
    assert!(parse(&format!("INSERT INTO t VALUES ({})", "1,".repeat(63) + "1")).is_ok());
    assert!(
        parse(&format!(
            "INSERT INTO t VALUES {}",
            "(1),".repeat(255) + "(1)"
        ))
        .is_ok()
    );
    assert_eq!(
        parse(&format!(
            "INSERT INTO t VALUES {}",
            "(1),".repeat(256) + "(1)"
        )),
        Err(Error::Limit("insert rows"))
    );
    assert!(parse(&format!("SELECT * FROM {}", "x".repeat(63))).is_ok());
    assert!(parse(&format!("SELECT * FROM {}", "x".repeat(64))).is_err());
    assert!(parse(&format!("INSERT INTO t VALUES ('{}')", "я".repeat(1536))).is_ok());
    assert_eq!(
        parse(&format!("INSERT INTO t VALUES ('{}')", "я".repeat(1537))),
        Err(Error::Limit("literal bytes"))
    );
    assert_eq!(
        parse(&";".repeat(MAX_TOKENS + 1)),
        Err(Error::Limit("tokens"))
    );
    assert_eq!(
        parse(&format!(
            "SELECT * FROM t WHERE {}TRUE",
            "NOT ".repeat(MAX_EXPRESSION_DEPTH)
        )),
        Err(Error::Limit("expression depth"))
    );
    assert_eq!(
        parse(&format!(
            "SELECT * FROM t WHERE {}TRUE{}",
            "(".repeat(MAX_EXPRESSION_DEPTH),
            ")".repeat(MAX_EXPRESSION_DEPTH)
        )),
        Err(Error::Limit("expression depth"))
    );
}

#[test]
fn combined_flat_chains_cannot_create_an_overdeep_ast() {
    let left = "TRUE AND ".repeat(20) + "TRUE";
    let expression = left + &" OR TRUE".repeat(20);
    assert_eq!(
        parse(&format!("SELECT * FROM t WHERE {expression}")),
        Err(Error::Limit("expression depth"))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn arbitrary_unicode_input_is_bounded_and_deterministic(text in ".{0,1200}") {
        prop_assert_eq!(parse(&text),parse(&text));
        for offset in text.char_indices().map(|(i,_)|i).step_by(31) { let _=parse(&text[..offset]); }
    }
    #[test]
    fn escaped_text_and_numbered_parameters_never_become_sql(text in "[^\x00]{0,100}", number in any::<i64>(), parameter in 1usize..257) {
        let escaped=text.replace('\'',"''");
        let parsed=parse(&format!("INSERT INTO t VALUES ({number},'{escaped}',${parameter})")).unwrap();
        let Statement::Insert {rows,..}=&parsed[0] else {panic!()};
        prop_assert_eq!(&rows[0],&vec![Scalar::Literal(Value::Integer(number)),Scalar::Literal(Value::Text(text)),Scalar::Parameter(parameter)]);
    }
}
