#![no_main]
use emilybase_auth::file_policy::{
    StoreContext,
    records::{encode, inspect},
};
use emilybase_catalog::{Value, decode_row, encode_row};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let context = StoreContext {
        project: "11111111111111111111111111111111",
        database: [3; 16],
    };
    if let Ok(row) = decode_row(data)
        && let Ok(decoded) = inspect(context, &row)
    {
        let encoded = encode(
            context,
            decoded.revision(),
            decoded.previous(),
            decoded.document(),
        )
        .unwrap();
        assert_eq!(encoded.row(), row);
        assert_eq!(encode_row(&row).unwrap(), data);
    }
    if let Ok(record) = encode(context, 9, 5, data) {
        let mut row = record.into_row();
        let decoded = inspect(context, &row).unwrap();
        assert_eq!(decoded.document(), data);
        let Value::Bytes(checksum) = &mut row[7] else {
            unreachable!()
        };
        checksum[0] ^= 1;
        assert!(inspect(context, &row).is_err());
    }
    // Pure grammar/typed records only, without invented identity or installation.
});
