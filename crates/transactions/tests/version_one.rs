use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_transactions::Database;
use emilybase_wal::{HEADER_SIZE, encode_header};
use sha2::{Digest, Sha256};

// Frozen from synthetic archives produced before WAL version 2 existed.
// Only the random database identity is normalized; all old frame/page bytes
// must remain identical, including their checksums and commit digest.
const ROOT: &str = "e80637c8e106ede856efa46bf661d45ed2bb7c69efef14fd35bc387118b6322f";
const TABLE: &str = "0a7bf09ce9d6f593e0ba0ae1d2753628a11af1a51b7689d2eddda277f6714638";

fn normalized_hash(mut bytes: Vec<u8>) -> String {
    assert_eq!(&bytes[..16], b"EMILYWAL\x01\0\0\x10\x40\x10\0\0");
    bytes[..HEADER_SIZE].copy_from_slice(&encode_header([7; 16]).unwrap());
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn original_version_one_root_and_table_images_remain_byte_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Database::create(&path).unwrap();
    assert_eq!(normalized_hash(db.committed_wal().unwrap()), ROOT);
    let mut tx = db.begin().unwrap();
    tx.create_table(Schema {
        name: "items".into(),
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
        ],
        primary_key: 0,
    })
    .unwrap();
    tx.insert(
        "items",
        vec![Value::Integer(7), Value::Text("synthetic seed".into())],
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(normalized_hash(db.committed_wal().unwrap()), TABLE);
    db.checkpoint().unwrap();
    drop(db);
    let mut db = Database::open(path).unwrap();
    assert_eq!(normalized_hash(db.committed_wal().unwrap()), TABLE);
}
