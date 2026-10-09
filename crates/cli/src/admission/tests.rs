use super::*;
use proptest::prelude::*;

#[test]
fn revisions_and_receipts_keep_exact_unsigned_digits_including_above_json_safe_integer() {
    for number in [0, 1, 9_007_199_254_740_993, u64::MAX] {
        assert_eq!(expected(&number.to_string()).unwrap(), number);
    }
    for text in [
        "",
        "+0",
        "-1",
        "00",
        "01",
        " 1",
        "1 ",
        "1\n",
        "1e0",
        "18446744073709551616",
        "synthetic-private-input",
    ] {
        assert_eq!(
            expected(text).unwrap_err().to_string(),
            "invalid canonical expected admission revision"
        );
    }
    let receipt = Receipt::from(PublicAdmissionReceipt {
        enabled: true,
        revision: u64::MAX,
        previous: 9_007_199_254_740_993,
    });
    assert_eq!(
        serde_json::to_value(receipt).unwrap(),
        serde_json::json!({
            "enabled":true,"revision":"18446744073709551615","previous":"9007199254740993"
        })
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn canonical_revision_conversion_preserves_all_generated_u64_values(number in any::<u64>()) {
        let digits=number.to_string();
        prop_assert_eq!(expected(&digits).unwrap(),number);
        for changed in [format!("+{digits}"),format!("0{digits}"),format!("{digits} ")] {
            prop_assert!(expected(&changed).is_err());
        }
    }
}
