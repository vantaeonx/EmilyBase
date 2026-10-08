#![no_main]
use emilybase_auth::row_policy::{BoundPolicy, TableContext, decode};
use emilybase_catalog::{Column, DataType, Schema};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(definition) = decode(data) {
        let schema = Schema {
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
        };
        // Grammar/compilation only; no fabricated principal, I/O, KDF or authority.
        let _ = BoundPolicy::compile(
            TableContext {
                project: "11111111111111111111111111111111",
                id: 1,
                schema: &schema,
            },
            &definition,
        );
    }
});
