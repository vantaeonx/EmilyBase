use super::*;
use emilybase_catalog::{Column, DataType};
use proptest::prelude::*;
const PROJECT: &str = "11111111111111111111111111111111";
const DOCUMENT:&[u8]=br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
fn schema() -> Schema {
    Schema {
        name: "items".into(),
        columns: vec![Column {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
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
fn rows() -> (Row, Vec<Row>) {
    encode(context(&schema()), 9, 5, DOCUMENT)
        .unwrap()
        .into_rows()
}
fn read(header: &[Value], chunks: &[Row]) -> Result<DecodedPolicy> {
    inspect(PROJECT, header, chunks.iter().map(Vec::as_slice))
}
#[test]
fn canonical_records_match_separate_python_digest_and_preserve_exact_definition_and_metadata() {
    let (header, chunks) = rows();
    let decoded = read(&header, &chunks).unwrap();
    assert_eq!(decoded.table, 7);
    assert_eq!(decoded.revision, 9);
    assert_eq!(decoded.previous, 5);
    assert_eq!(decoded.schema, schema());
    assert_eq!(decoded.document(), DOCUMENT);
    let digest = decoded
        .sha256
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "da6eaa0f1b45d8ad23d03c0052e1f351415fbad8569c8abe24964a0b0393fd83"
    );
    let bytes = encode_schema(&schema()).unwrap();
    assert_eq!(bytes, b"ESCH\x01\0\0\0\x01\0\x05\0items\x02\0id\x02\0");
    header_schema().validate_row(&header).unwrap();
    for row in &chunks {
        chunk_schema().validate_row(row).unwrap();
        assert!(encode_row(row).unwrap().len() <= MAX_ENCODED_BYTES);
    }
    let mut spaced = DOCUMENT.to_vec();
    spaced.extend_from_slice(b" \n");
    let different = encode(context(&schema()), 9, 5, &spaced).unwrap();
    let decoded2 = inspect(PROJECT, different.header(), different.chunks()).unwrap();
    assert_ne!(decoded.sha256, decoded2.sha256);
    assert_eq!(decoded2.document(), spaced);
}
#[test]
fn metadata_uses_full_canonical_u64_and_refuses_bad_revision_and_scope_before_fragmenting() {
    let records = encode(
        TableContext {
            id: u64::MAX,
            ..context(&schema())
        },
        u64::MAX,
        u64::MAX - 1,
        DOCUMENT,
    )
    .unwrap();
    let decoded = inspect(PROJECT, records.header(), records.chunks()).unwrap();
    assert_eq!(decoded.table, u64::MAX);
    assert_eq!(decoded.revision, u64::MAX);
    assert_eq!(decoded.previous, u64::MAX - 1);
    for (revision, previous) in [(0, 0), (1, 0), (2, 1), (2, 2), (3, 4)] {
        assert!(encode(context(&schema()), revision, previous, DOCUMENT).is_err());
    }
    assert!(
        encode(
            TableContext {
                project: "../outside",
                ..context(&schema())
            },
            9,
            5,
            DOCUMENT
        )
        .is_err()
    );
    assert!(
        encode(
            TableContext {
                id: 0,
                ..context(&schema())
            },
            9,
            5,
            DOCUMENT
        )
        .is_err()
    );
    let (header, chunks) = rows();
    assert!(matches!(
        inspect(
            "22222222222222222222222222222222",
            &header,
            chunks.iter().map(Vec::as_slice)
        ),
        Err(PolicyError::Scope)
    ));
}
#[test]
fn largest_document_and_near_maximum_schema_fit_seven_normal_slotted_records() {
    let mut schema = schema();
    schema.columns.extend((0..62).map(|i| Column {
        name: format!("c{i:02}{}", "x".repeat(55)),
        data_type: DataType::Text,
        nullable: true,
    }));
    let mut document = DOCUMENT.to_vec();
    document.resize(MAX_DOCUMENT_BYTES, b' ');
    let records = encode(context(&schema), 9, 0, &document).unwrap();
    assert_eq!(records.chunks().len(), MAX_POLICY_CHUNKS);
    assert_eq!(MAX_POLICY_CHUNKS, 7);
    let decoded = inspect(PROJECT, records.header(), records.chunks()).unwrap();
    assert_eq!(decoded.schema, schema);
    assert_eq!(decoded.document(), document);
    for (i, row) in records.chunks().enumerate() {
        assert!(encode_row(row).unwrap().len() <= 4000);
        if i < 6 {
            assert!(matches!(&row[1],Value::Bytes(bytes) if bytes.len()==3072));
        }
    }
    document.push(b' ');
    assert!(encode(context(&schema), 10, 9, &document).is_err());
}
#[test]
fn altered_header_fields_bad_canonical_decimals_lengths_and_checksums_are_refused() {
    let (header, chunks) = rows();
    for (index, values) in [
        (
            0,
            vec![
                Value::Text("07".into()),
                Value::Text("0".into()),
                Value::Text("18446744073709551616".into()),
                Value::Integer(7),
            ],
        ),
        (1, vec![Value::Integer(0), Value::Integer(2)]),
        (
            3,
            vec![
                Value::Text("1".into()),
                Value::Text("+9".into()),
                Value::Text("9\n".into()),
            ],
        ),
        (
            4,
            vec![
                Value::Text("1".into()),
                Value::Text("9".into()),
                Value::Text("05".into()),
            ],
        ),
        (
            5,
            vec![
                Value::Integer(-1),
                Value::Integer(0),
                Value::Integer(4001),
                Value::Integer(i64::MAX),
            ],
        ),
        (
            6,
            vec![Value::Integer(-1), Value::Integer(0), Value::Integer(16385)],
        ),
        (
            7,
            vec![
                Value::Bytes(vec![]),
                Value::Bytes(vec![0; 32]),
                Value::Bytes(vec![0; 33]),
            ],
        ),
    ] {
        for value in values {
            let mut bad = header.clone();
            bad[index] = value;
            assert!(read(&bad, &chunks).is_err());
        }
    }
    let mut bad = header.clone();
    bad.push(Value::Null);
    assert!(read(&bad, &chunks).is_err());
    assert!(read(&header[..7], &chunks).is_err());
}
#[test]
fn missing_extra_reordered_noncanonical_duplicate_and_wrong_length_chunks_are_refused() {
    let mut document = DOCUMENT.to_vec();
    document.resize(7000, b' ');
    let (header, chunks) = encode(context(&schema()), 9, 5, &document)
        .unwrap()
        .into_rows();
    assert_eq!(chunks.len(), 3);
    assert!(read(&header, &chunks[..2]).is_err());
    let mut bad = chunks.clone();
    bad.push(chunks[0].clone());
    assert!(read(&header, &bad).is_err());
    let mut bad = chunks.clone();
    bad.swap(0, 1);
    assert!(read(&header, &bad).is_err());
    let mut bad = chunks.clone();
    bad[1] = bad[0].clone();
    assert!(read(&header, &bad).is_err());
    for value in [
        Value::Text("07:0".into()),
        Value::Text("7:00".into()),
        Value::Text("8:0".into()),
        Value::Integer(0),
    ] {
        let mut bad = chunks.clone();
        bad[0][0] = value;
        assert!(read(&header, &bad).is_err());
    }
    for length in [0, 3071, 3073, 4000] {
        let mut bad = chunks.clone();
        bad[0][1] = Value::Bytes(vec![0; length]);
        assert!(read(&header, &bad).is_err());
    }
    let mut bad = chunks.clone();
    bad[0].push(Value::Null);
    assert!(read(&header, &bad).is_err());
}
fn repaired(header: &mut [Value], chunks: &[Row]) {
    let body = chunks
        .iter()
        .flat_map(|row| match &row[1] {
            Value::Bytes(bytes) => bytes.clone(),
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    let (Value::Integer(schema_len), Value::Integer(document_len)) = (&header[5], &header[6])
    else {
        unreachable!()
    };
    header[7] = Value::Bytes(
        digest(
            PROJECT,
            7,
            9,
            5,
            *schema_len as usize,
            *document_len as usize,
            &body,
        )
        .to_vec(),
    );
}
#[test]
fn recomputed_checksum_cannot_make_invalid_nested_schema_document_or_policy_valid() {
    let (mut header, mut chunks) = rows();
    let Value::Bytes(bytes) = &mut chunks[0][1] else {
        unreachable!()
    };
    bytes[0] ^= 1;
    repaired(&mut header, &chunks);
    assert!(read(&header, &chunks).is_err());
    let (mut header, mut chunks) = rows();
    let Value::Bytes(bytes) = &mut chunks[0][1] else {
        unreachable!()
    };
    let schema_len = encode_schema(&schema()).unwrap().len();
    bytes[schema_len] = b'!';
    repaired(&mut header, &chunks);
    assert!(read(&header, &chunks).is_err());
    let (mut header, mut chunks) = rows();
    let Value::Bytes(bytes) = &mut chunks[0][1] else {
        unreachable!()
    };
    let position = bytes
        .windows(11)
        .position(|s| s == b"\"version\":1")
        .unwrap();
    bytes[position + 10] = b'2';
    repaired(&mut header, &chunks);
    assert!(matches!(read(&header, &chunks), Err(PolicyError::Version)));
}
#[test]
fn record_debug_and_errors_redact_documents_schema_and_project() {
    let records = encode(context(&schema()), 9, 5, DOCUMENT).unwrap();
    let decoded = inspect(PROJECT, records.header(), records.chunks()).unwrap();
    for text in [
        format!("{records:?}"),
        format!("{decoded:?}"),
        PolicyError::Document.to_string(),
    ] {
        assert!(!text.contains(PROJECT));
        assert!(!text.contains("version"));
        assert!(!text.contains("items"));
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn every_mutated_payload_byte_is_detected(position in any::<usize>(),bit in 0u8..8) {
        let (header,mut chunks)=rows();let Value::Bytes(bytes)=&mut chunks[0][1] else {unreachable!()};let position=position%bytes.len();bytes[position]^=1<<bit;prop_assert!(read(&header,&chunks).is_err());
    }
    #[test]
    fn bounded_padding_and_revision_gaps_preserve_exact_records(padding in 0usize..16000,revision in 2u64..u64::MAX) {
        let mut document=DOCUMENT.to_vec();document.extend(std::iter::repeat_n(b' ',padding));let records=encode(context(&schema()),revision,0,&document).unwrap();let decoded=inspect(PROJECT,records.header(),records.chunks()).unwrap();prop_assert_eq!(decoded.document(),document);prop_assert_eq!(decoded.revision,revision);
    }
}
