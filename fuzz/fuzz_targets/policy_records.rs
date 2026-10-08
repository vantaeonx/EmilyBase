#![no_main]
use emilybase_auth::row_policy::{
    TableContext,
    records::{encode, inspect},
};
use emilybase_catalog::{Column, DataType, Row, Schema, Value};
use libfuzzer_sys::fuzz_target;
const PROJECT: &str = "11111111111111111111111111111111";
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    if let Ok((header, chunks)) = serde_json::from_slice::<(Row, Vec<Row>)>(data) {
        let _ = inspect(PROJECT, &header, chunks.iter().map(Vec::as_slice));
    }
    let schema = Schema {
        name: "items".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        primary_key: 0,
    };
    if let Ok(records) = encode(
        TableContext {
            project: PROJECT,
            id: 7,
            schema: &schema,
        },
        9,
        5,
        data,
    ) {
        let decoded =
            inspect(PROJECT, records.header(), records.chunks()).expect("encoded complete records");
        assert_eq!(decoded.document(), data);
        assert_eq!(decoded.schema, schema);
        assert_eq!(decoded.revision, 9);
        let (mut header, mut chunks) = records.into_rows();
        if let Value::Bytes(checksum) = &mut header[7] {
            checksum[0] ^= 1;
        } else {
            unreachable!()
        }
        assert!(inspect(PROJECT, &header, chunks.iter().map(Vec::as_slice)).is_err());
        if let Value::Bytes(checksum) = &mut header[7] {
            checksum[0] ^= 1;
        } else {
            unreachable!()
        }
        let Value::Bytes(bytes) = &mut chunks[0][1] else {
            unreachable!()
        };
        bytes[0] ^= 1;
        assert!(inspect(PROJECT, &header, chunks.iter().map(Vec::as_slice)).is_err());
    }
});
