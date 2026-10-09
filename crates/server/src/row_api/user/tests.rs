use super::*;
#[test]
fn user_row_transport_supports_only_get_page_and_typed_packet_grammar() {
    for (op, bytes) in [(Operation::Get,br#"{"table":"t","key":{"type":"integer","value":"9223372036854775807"}}"#.as_slice()), (Operation::Page,br#"{"table":"t","after":null,"limit":128}"#.as_slice()),(Operation::Batch,br#"{"table":"t","operations":[{"op":"delete","key":{"type":"integer","value":"-9223372036854775808"}}]}"#.as_slice())] { assert!(validate_user_row_request(op,bytes).is_ok()); }
    for (op, bytes) in [
        (Operation::Insert,br#"{"table":"t","row":[{"type":"integer","value":"1"}]}"#.as_slice()),
        (Operation::Update,br#"{"table":"t","key":{"type":"integer","value":"1"},"row":[{"type":"integer","value":"1"}]}"#.as_slice()),
        (Operation::Delete,br#"{"table":"t","key":{"type":"integer","value":"1"}}"#.as_slice())
    ] { assert!(validate_row_request(op,bytes).is_ok()); assert!(validate_user_row_request(op,bytes).is_err()); }
    for bytes in [
        br#"{"table":"t","after":null,"limit":0}"#.as_slice(),
        br#"{"table":"t","after":null,"limit":129}"#.as_slice(),
        br#"{"table":"t","after":null,"limit":1,"time":50}"#.as_slice(),
        br#"{"table":"t","table":"t","limit":1}"#.as_slice(),
    ] {
        assert!(validate_user_row_request(Operation::Page, bytes).is_err());
    }
}
#[tokio::test]
async fn user_row_transport_preserves_full_digits_and_bounds_complete_responses() {
    let result = Out::Committed {
        transaction: u64::MAX,
        operations: 256,
    };
    let response = user_response(&result).unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), crate::http::MAX_BODY)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"changed":256,"transaction":"18446744073709551615"})
    );
    let row = vec![emilybase_catalog::Value::Text("界".repeat(1024))];
    assert!(matches!(
        user_response(&Out::Page {
            rows: vec![row; 128],
            next: None
        }),
        Err(UserRowTransportError::Response)
    ));
}
