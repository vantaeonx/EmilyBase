use std::fs;

use emilybase_backup::{Error, HEADER_SIZE, encode, inspect_bytes, restore};
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind};
use emilybase_transactions::Database;
use emilybase_wal::{Wal, recover};
use sha2::{Digest, Sha256};

fn schema(name: &str) -> Schema {
    Schema {
        name: name.into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    }
}

fn event(kind: EventKind) -> Vec<u8> {
    Event { table_id: 1, kind }.encode().unwrap()
}

#[test]
fn complete_checksums_do_not_authorize_invalid_relational_history_in_restore() {
    for scenario in ["rewrite", "duplicate", "dropped", "type"] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let mut db = Database::create(&source).unwrap();
        let mut tx = db.begin().unwrap();
        tx.create_table(schema("items")).unwrap();
        tx.insert("items", vec![Value::Integer(1)]).unwrap();
        tx.commit().unwrap();
        let template = encode(&db.committed_wal().unwrap()).unwrap();
        let database_id = db.database_id();
        drop(db);
        let (mut wal, recovered) = Wal::open(source.join("redo.wal"), None).unwrap();
        let mut page = recovered.committed.last().unwrap().pages[0].clone();
        match scenario {
            "rewrite" => {
                page.update(1, &event(EventKind::Create(schema("rewritten"))))
                    .unwrap();
            }
            "duplicate" => {
                page.insert(&event(EventKind::Insert(vec![Value::Integer(1)])))
                    .unwrap();
            }
            "dropped" => {
                page.insert(&event(EventKind::Drop)).unwrap();
                page.insert(&event(EventKind::Insert(vec![Value::Integer(2)])))
                    .unwrap();
            }
            "type" => {
                page.insert(&event(EventKind::Insert(vec![Value::Text("wrong".into())])))
                    .unwrap();
            }
            _ => unreachable!(),
        }
        wal.append(&[page]).unwrap();
        drop(wal);
        let payload = fs::read(source.join("redo.wal")).unwrap();
        // All WAL/page CRCs and commit digests pass independently of table replay.
        assert_eq!(
            recover(&payload, Some(database_id))
                .unwrap()
                .committed
                .len(),
            3
        );
        assert!(encode(&payload).is_err(), "{scenario}");
        let mut archive = template[..HEADER_SIZE].to_vec();
        archive[32..40].copy_from_slice(&3u64.to_le_bytes());
        archive[40..48].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        archive[48..80].copy_from_slice(&Sha256::digest(&payload));
        let crc = crc32fast::hash(&archive[..124]);
        archive[124..128].copy_from_slice(&crc.to_le_bytes());
        archive.extend_from_slice(&payload);
        assert!(
            matches!(inspect_bytes(&archive), Err(Error::Transactions(_))),
            "{scenario}"
        );
        let path = dir.path().join("invalid.backup");
        fs::write(&path, &archive).unwrap();
        let destination = dir.path().join("restored");
        assert!(restore(&path, &destination).is_err(), "{scenario}");
        assert!(!destination.exists());
        assert_eq!(fs::read(path).unwrap(), archive);
        assert_eq!(fs::read(source.join("redo.wal")).unwrap(), payload);
        assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".emilybase-backup-")
        }));
    }
}
