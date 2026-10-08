use super::*;
use crate::row_policy::{BoundPolicy, Change, Definition, PolicyError, Rule, TableContext};
use emilybase_catalog::{Column, DataType, Schema};
use proptest::prelude::*;
const PROJECT: &str = "11111111111111111111111111111111";
fn schema() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "owner".into(),
                data_type: DataType::Bytes,
                nullable: true,
            },
            Column {
                name: "visible".into(),
                data_type: DataType::Boolean,
                nullable: false,
            },
            Column {
                name: "note".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
        primary_key: 0,
    }
}
fn context(schema: &Schema) -> TableContext<'_> {
    TableContext {
        project: PROJECT,
        id: 7,
        schema,
    }
}
fn owner() -> Rule {
    Rule::Owner {
        column: "owner".into(),
    }
}
fn policy(select: Rule) -> Definition {
    Definition {
        version: 1,
        select,
        insert: owner(),
        update_using: owner(),
        update_check: owner(),
        delete: owner(),
    }
}
fn row(owner: [u8; 16], visible: bool) -> Vec<Value> {
    vec![
        Value::Integer(1),
        Value::Bytes(owner.to_vec()),
        Value::Boolean(visible),
        Value::Null,
    ]
}
fn opened(path: &Path, project: &str) -> (AccountStore, AccountInfo, IssuedSession) {
    let mut store = AccountStore::create(path, project, PasswordPool::new(1).unwrap()).unwrap();
    let info = store
        .create_user("synthetic", b"synthetic-password")
        .unwrap();
    store.enable_session_clock(100).unwrap();
    let session = store
        .sign_in("synthetic", b"synthetic-password", 100)
        .unwrap();
    (store, info, session)
}
#[test]
fn borrowed_current_proof_enforces_old_and_new_ownership_without_private_writes() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mut store, info, session) = opened(&dir.path().join("private"), PROJECT);
    let s = schema();
    let definition = policy(Rule::Any {
        terms: vec![
            owner(),
            Rule::Equal {
                column: "visible".into(),
                value: Value::Boolean(true),
            },
        ],
    });
    let bound = BoundPolicy::compile(context(&s), &definition).unwrap();
    let own = row(info.id, false);
    let mut foreign_id = info.id;
    foreign_id[0] ^= 1;
    let foreign = row(foreign_id, false);
    let visible = row(foreign_id, true);
    let before = store.database.committed_wal().unwrap();
    {
        let principal = store.verify_access(session.access.expose(), 100).unwrap();
        assert!(
            bound
                .authorize(context(&s), &principal, Change::Select(&own))
                .is_ok()
        );
        assert!(
            bound
                .authorize(context(&s), &principal, Change::Select(&visible))
                .is_ok()
        );
        assert!(matches!(
            bound.authorize(context(&s), &principal, Change::Select(&foreign)),
            Err(PolicyError::Denied)
        ));
        for change in [
            Change::Insert(&own),
            Change::Delete(&own),
            Change::Update {
                old: &own,
                new: &own,
            },
        ] {
            assert!(bound.authorize(context(&s), &principal, change).is_ok());
        }
        for change in [
            Change::Insert(&foreign),
            Change::Delete(&foreign),
            Change::Update {
                old: &own,
                new: &foreign,
            },
            Change::Update {
                old: &foreign,
                new: &own,
            },
        ] {
            assert!(matches!(
                bound.authorize(context(&s), &principal, change),
                Err(PolicyError::Denied)
            ));
        }
    }
    assert_eq!(store.database.committed_wal().unwrap(), before);
    store.logout_session(session.refresh.expose(), 101).unwrap();
    assert!(store.verify_access(session.access.expose(), 101).is_err());
}
#[test]
fn even_allow_all_cannot_cross_project_table_recreation_schema_or_invalid_rows() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mut store, info, session) = opened(&dir.path().join("private"), PROJECT);
    let s = schema();
    let values = row(info.id, true);
    let definition = Definition {
        version: 1,
        select: Rule::Authenticated {},
        insert: Rule::Authenticated {},
        update_using: Rule::Authenticated {},
        update_check: Rule::Authenticated {},
        delete: Rule::Authenticated {},
    };
    let bound = BoundPolicy::compile(context(&s), &definition).unwrap();
    let principal = store.verify_access(session.access.expose(), 100).unwrap();
    let mut changed = s.clone();
    changed.columns[2].name = "renamed".into();
    for context in [
        TableContext {
            project: "22222222222222222222222222222222",
            ..context(&s)
        },
        TableContext {
            id: 8,
            ..context(&s)
        },
        TableContext {
            schema: &changed,
            ..context(&s)
        },
    ] {
        assert!(matches!(
            bound.authorize(context, &principal, Change::Select(&values)),
            Err(PolicyError::Scope)
        ));
    }
    for bad in [
        vec![],
        vec![Value::Integer(1)],
        vec![
            Value::Integer(1),
            Value::Bytes(vec![0; 16]),
            Value::Text("invalid".into()),
            Value::Null,
        ],
        vec![
            Value::Integer(1),
            Value::Bytes(vec![0; 3072]),
            Value::Boolean(true),
            Value::Text("x".repeat(3072)),
        ],
    ] {
        assert!(matches!(
            bound.authorize(context(&s), &principal, Change::Select(&bad)),
            Err(PolicyError::Row)
        ));
    }
    let mut new = values.clone();
    new[0] = Value::Integer(2);
    assert!(matches!(
        bound.authorize(
            context(&s),
            &principal,
            Change::Update {
                old: &values,
                new: &new
            }
        ),
        Err(PolicyError::Row)
    ));
    drop(principal);
    drop(store);
    let (mut other, _, session) = opened(
        &dir.path().join("other"),
        "22222222222222222222222222222222",
    );
    let principal = other.verify_access(session.access.expose(), 100).unwrap();
    assert!(matches!(
        bound.authorize(context(&s), &principal, Change::Select(&values)),
        Err(PolicyError::Scope)
    ));
}
#[test]
fn malformed_owner_null_literals_and_independent_generated_decisions_fail_closed() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mut store, info, session) = opened(&dir.path().join("private"), PROJECT);
    let s = schema();
    let definition = policy(Rule::All {
        terms: vec![
            Rule::Any {
                terms: vec![
                    owner(),
                    Rule::Equal {
                        column: "visible".into(),
                        value: Value::Boolean(true),
                    },
                ],
            },
            Rule::IsNull {
                column: "note".into(),
            },
        ],
    });
    let bound = BoundPolicy::compile(context(&s), &definition).unwrap();
    let principal = store.verify_access(session.access.expose(), 100).unwrap();
    let mut foreign_id = info.id;
    foreign_id[0] ^= 1;
    let strategy = (any::<bool>(), any::<bool>(), any::<bool>(), 0usize..20);
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(128));
    runner
        .run(&strategy, |(owned, visible, null, owner_size)| {
            let owner_value = if owned {
                Value::Bytes(info.id.to_vec())
            } else if owner_size == 0 {
                Value::Null
            } else {
                Value::Bytes(
                    foreign_id
                        .iter()
                        .copied()
                        .cycle()
                        .take(owner_size)
                        .collect(),
                )
            };
            let values = vec![
                Value::Integer(1),
                owner_value,
                Value::Boolean(visible),
                if null {
                    Value::Null
                } else {
                    Value::Text("synthetic".into())
                },
            ];
            prop_assert_eq!(
                bound
                    .authorize(context(&s), &principal, Change::Select(&values))
                    .is_ok(),
                (owned || visible) && null
            );
            Ok(())
        })
        .unwrap();
    let deny = BoundPolicy::compile(
        context(&s),
        &Definition {
            version: 1,
            select: Rule::Deny {},
            insert: Rule::Deny {},
            update_using: Rule::Deny {},
            update_check: Rule::Deny {},
            delete: Rule::Deny {},
        },
    )
    .unwrap();
    let values = row(info.id, true);
    for change in [
        Change::Select(&values),
        Change::Insert(&values),
        Change::Delete(&values),
        Change::Update {
            old: &values,
            new: &values,
        },
    ] {
        assert!(matches!(
            deny.authorize(context(&s), &principal, change),
            Err(PolicyError::Denied)
        ));
    }
}

#[test]
fn decisions_compose_inside_original_transactions_and_old_bindings_refuse_real_table_recreation() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (mut private, info, session) = opened(&dir.path().join("private"), PROJECT);
    let before_private = private.database.committed_wal().unwrap();
    let principal = private.verify_access(session.access.expose(), 100).unwrap();
    for format in [1, 2] {
        let path = dir.path().join(format!("data-{format}"));
        let mut data = Database::create(&path).unwrap();
        if format == 2 {
            data.compact().unwrap();
        }
        let mut tx = data.begin().unwrap();
        tx.create_table(schema()).unwrap();
        tx.commit().unwrap();
        let snapshot = data.view().unwrap();
        let table = snapshot.table_id("items").unwrap();
        let bound = BoundPolicy::compile(
            TableContext {
                project: PROJECT,
                id: table,
                schema: snapshot.schema("items").unwrap(),
            },
            &policy(owner()),
        )
        .unwrap();
        let own = row(info.id, false);
        let mut tx = data.begin().unwrap();
        let snapshot = tx.view().unwrap();
        bound
            .authorize(
                TableContext {
                    project: PROJECT,
                    id: snapshot.table_id("items").unwrap(),
                    schema: snapshot.schema("items").unwrap(),
                },
                &principal,
                Change::Insert(&own),
            )
            .unwrap();
        tx.insert("items", own.clone()).unwrap();
        tx.commit().unwrap();
        let before = data.committed_wal().unwrap();
        let mut wrong = own.clone();
        wrong[1] = Value::Bytes(vec![0; 15]);
        let denied = (|| -> std::result::Result<(), Box<dyn std::error::Error>> {
            let mut tx = data.begin()?;
            let mut prefix = own.clone();
            prefix[0] = Value::Integer(2);
            tx.insert("items", prefix)?;
            let snapshot = tx.view()?;
            let old = snapshot.get("items", &Key::Integer(1))?.unwrap();
            bound.authorize(
                TableContext {
                    project: PROJECT,
                    id: snapshot.table_id("items")?,
                    schema: snapshot.schema("items")?,
                },
                &principal,
                Change::Update { old, new: &wrong },
            )?;
            tx.update("items", &Key::Integer(1), wrong)?;
            tx.commit()?;
            Ok(())
        })();
        assert!(denied.is_err());
        assert_eq!(data.committed_wal().unwrap(), before);
        assert!(
            data.view()
                .unwrap()
                .get("items", &Key::Integer(2))
                .unwrap()
                .is_none()
        );
        drop(data);
        let mut data = Database::open(&path).unwrap();
        assert_eq!(
            data.view().unwrap().get("items", &Key::Integer(1)).unwrap(),
            Some(&own)
        );
        let mut tx = data.begin().unwrap();
        tx.drop_table("items").unwrap();
        tx.create_table(schema()).unwrap();
        tx.commit().unwrap();
        let snapshot = data.view().unwrap();
        let new_id = snapshot.table_id("items").unwrap();
        assert_ne!(new_id, table);
        assert!(matches!(
            bound.authorize(
                TableContext {
                    project: PROJECT,
                    id: new_id,
                    schema: snapshot.schema("items").unwrap()
                },
                &principal,
                Change::Insert(&own)
            ),
            Err(PolicyError::Scope)
        ));
    }
    drop(principal);
    assert_eq!(private.database.committed_wal().unwrap(), before_private);
}

#[test]
fn policy_records_survive_original_atomic_replace_compact_and_verified_restore_on_both_wals() {
    use crate::row_policy::records::{chunk_schema, encode, header_schema, inspect};
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let schema = schema();
    let mut document=br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#.to_vec();
    document.resize(crate::row_policy::MAX_DOCUMENT_BYTES, b' ');
    for format in [1, 2] {
        let path = dir.path().join(format!("records-{format}"));
        let mut data = Database::create(&path).unwrap();
        if format == 2 {
            data.compact().unwrap();
        }
        let mut tx = data.begin().unwrap();
        tx.create_table(header_schema()).unwrap();
        tx.create_table(chunk_schema()).unwrap();
        tx.commit().unwrap();
        let original = data.last_transaction() + 1;
        let records = encode(context(&schema), original, 0, &document).unwrap();
        let mut tx = data.begin().unwrap();
        tx.insert(&header_schema().name, records.header().to_vec())
            .unwrap();
        for row in records.chunks() {
            tx.insert(&chunk_schema().name, row.to_vec()).unwrap();
        }
        tx.commit().unwrap();
        assert_eq!(data.last_transaction(), original);
        let before = data.committed_wal().unwrap();
        let mut changed = document.clone();
        *changed.last_mut().unwrap() = b'\n';
        let next = data.last_transaction() + 1;
        let replacement = encode(context(&schema), next, original, &changed).unwrap();
        {
            let mut tx = data.begin().unwrap();
            tx.update(
                &header_schema().name,
                &Key::Text("7".into()),
                replacement.header().to_vec(),
            )
            .unwrap();
            let first = replacement.chunks().next().unwrap();
            tx.update(
                &chunk_schema().name,
                &Key::Text("7:0".into()),
                first.to_vec(),
            )
            .unwrap();
            // Discarding a partial staged record group publishes no metadata/chunks.
        }
        assert_eq!(data.committed_wal().unwrap(), before);
        drop(data);
        let mut data = Database::open(&path).unwrap();
        let read = |database: &Database| {
            let snapshot = database.view().unwrap();
            let header = snapshot
                .get(&header_schema().name, &Key::Text("7".into()))
                .unwrap()
                .unwrap();
            let chunks = snapshot
                .primary_rows(&chunk_schema().name, None, None)
                .unwrap()
                .map(|row| row.unwrap().as_slice())
                .collect::<Vec<_>>();
            inspect(PROJECT, header, chunks).unwrap()
        };
        let decoded = read(&data);
        assert_eq!(decoded.document(), document);
        assert_eq!(decoded.revision, original);
        let mut tx = data.begin().unwrap();
        tx.update(
            &header_schema().name,
            &Key::Text("7".into()),
            replacement.header().to_vec(),
        )
        .unwrap();
        for row in replacement.chunks() {
            let Value::Text(key) = &row[0] else {
                unreachable!()
            };
            tx.update(&chunk_schema().name, &Key::Text(key.clone()), row.to_vec())
                .unwrap();
        }
        tx.commit().unwrap();
        assert_eq!(data.last_transaction(), next);
        assert_eq!(read(&data).document(), changed);
        data.compact().unwrap();
        let archive = dir.path().join(format!("archive-{format}"));
        emilybase_backup::create(&mut data, &archive).unwrap();
        emilybase_backup::inspect(&archive).unwrap();
        let clone = dir.path().join(format!("clone-{format}"));
        emilybase_backup::restore(&archive, &clone).unwrap();
        let clone = Database::open(clone).unwrap();
        let decoded = read(&clone);
        assert_eq!(decoded.document(), changed);
        assert_eq!(decoded.revision, next);
        assert_eq!(decoded.previous, original);
        let mut damaged_header = replacement.header().to_vec();
        damaged_header[5] = Value::Integer(4001);
        let mut tx = data.begin().unwrap();
        tx.update(
            &header_schema().name,
            &Key::Text("7".into()),
            damaged_header,
        )
        .unwrap();
        tx.commit().unwrap();
        let snapshot = data.view().unwrap();
        let header = snapshot
            .get(&header_schema().name, &Key::Text("7".into()))
            .unwrap()
            .unwrap();
        let chunks = snapshot
            .primary_rows(&chunk_schema().name, None, None)
            .unwrap()
            .map(|r| r.unwrap().as_slice())
            .collect::<Vec<_>>();
        assert!(inspect(PROJECT, header, chunks).is_err());
        assert_eq!(read(&clone).document(), changed);
    }
}
