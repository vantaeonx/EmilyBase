use super::*;
use serde_json::{Value, json};
const DENY: &str = r#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#;
fn input(expected: &str, document: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"table":"items","expected":expected,"document":document})).unwrap()
}
#[test]
fn strict_request_preserves_exact_document_and_full_revision_digits_without_input_echo() {
    let mut long = DENY.to_owned();
    long.push_str(&" ".repeat(16_384 - long.len()));
    for expected in [0, 2, 9_007_199_254_740_993, u64::MAX] {
        let bytes = input(&expected.to_string(), &long);
        validate_policy_install_request(&bytes).unwrap();
        let (decoded, actual) = decode(&bytes).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(decoded.document.as_bytes(), long.as_bytes());
    }
    let mut over = long;
    over.push(' ');
    for bytes in [input("00",DENY),input("+2",DENY),input("-1",DENY),input("18446744073709551616",DENY),input("0",&over),input("0","synthetic-private-definition"),vec![b' ';crate::http::MAX_BODY+1],br#"{"table":"items","expected":"0","document":"\ud800"}"#.to_vec(),br#"{"table":"items","expected":"0","expected":"2","document":"synthetic-private-definition"}"#.to_vec()] {
        let error=validate_policy_install_request(&bytes).unwrap_err();
        assert_eq!(error.to_string(),"invalid bounded policy request document");
    }
    for field in ["schema", "project", "id", "now", "role"] {
        let mut value: Value = serde_json::from_slice(&input("0", DENY)).unwrap();
        value[field] = json!("synthetic-private-field");
        assert!(validate_policy_install_request(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
#[test]
fn wire_inventory_is_bounded_and_uses_decimal_strings_for_every_u64() {
    let receipt = PolicyReceipt {
        table: u64::MAX,
        revision: u64::MAX,
        previous: u64::MAX - 1,
        sha256: [255; 32],
    };
    let value = serde_json::to_value(Receipt::from(receipt)).unwrap();
    assert_eq!(value["table"], u64::MAX.to_string());
    assert_eq!(value["revision"], u64::MAX.to_string());
    assert_eq!(value["previous"], (u64::MAX - 1).to_string());
    assert_eq!(value["sha256"], "ff".repeat(32));
    let rows = (0..emilybase_auth::accounts::MAX_ROW_POLICIES)
        .map(|_| value.clone())
        .collect::<Vec<_>>();
    assert!(serde_json::to_vec(&json!({"policies":rows})).unwrap().len() < crate::http::MAX_BODY);
}
#[test]
fn failure_classification_never_calls_an_ambiguous_policy_write_a_rejected_request() {
    use emilybase_transactions::Error as T;
    for error in [
        Error::Policies(PolicyTransportError::Response),
        Error::Accounts(AccountError::Storage(T::Poisoned)),
        Error::Accounts(AccountError::Storage(T::Io(std::io::Error::other(
            "synthetic",
        )))),
    ] {
        let result = failure(error);
        assert_eq!(result.0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(result.1, "policy_outcome_requires_inspection");
    }
    for (error, status, code) in [
        (
            AccountError::PolicySchema,
            StatusCode::CONFLICT,
            "policy_catalog_disabled",
        ),
        (
            AccountError::PolicyConflict,
            StatusCode::CONFLICT,
            "policy_revision_conflict",
        ),
        (
            AccountError::PolicyCapacity,
            StatusCode::CONFLICT,
            "policy_capacity",
        ),
        (
            AccountError::Corrupt,
            StatusCode::SERVICE_UNAVAILABLE,
            "policy_catalog_invalid",
        ),
    ] {
        let result = failure(Error::Accounts(error));
        assert_eq!(result.0, status);
        assert_eq!(result.1, code);
    }
}
