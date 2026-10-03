use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind, MAX_ROWS, Snapshot};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn initialized(keys: impl IntoIterator<Item = String>) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Text,
                    nullable: false,
                }],
                primary_key: 0,
            }),
        })
        .unwrap();
    for text in keys {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Text(text)]),
            })
            .unwrap();
    }
    snapshot
}
fn selected(
    snapshot: &Snapshot,
    lower: Option<&str>,
    upper: Option<&str>,
    limit: usize,
) -> Vec<String> {
    snapshot
        .scan_text_range("t", lower, upper, limit)
        .unwrap()
        .into_iter()
        .map(|row| match row.into_iter().next().unwrap() {
            Value::Text(text) => text,
            _ => panic!("wrong type"),
        })
        .collect()
}

#[test]
fn text_intervals_merge_short_and_long_keys_before_applying_the_limit() {
    let keys = ["", "\0", "a", "a\0", "ab", "b", "界", "😀"]
        .map(str::to_owned)
        .into_iter()
        .chain([
            format!("a{}", "z".repeat(255)),
            format!("a{}", "z".repeat(256)),
            format!("a{}", "z".repeat(3071)),
        ])
        .collect::<BTreeSet<_>>();
    let snapshot = initialized(keys.iter().cloned());
    let before = snapshot.page_fingerprint();
    for lower in [None, Some(""), Some("a"), Some("b"), Some("界"), Some("😀")] {
        for upper in [None, Some(""), Some("a"), Some("b"), Some("界"), Some("😀")] {
            for limit in [0, 1, 2, 7, MAX_ROWS] {
                let expected = keys
                    .iter()
                    .filter(|key| {
                        lower.is_none_or(|lower| key.as_str() >= lower)
                            && upper.is_none_or(|upper| key.as_str() < upper)
                    })
                    .take(limit)
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(selected(&snapshot, lower, upper, limit), expected);
            }
        }
    }
    assert_eq!(snapshot.page_fingerprint(), before);
    assert_eq!(
        snapshot.primary_index_info("t").unwrap().excluded_long_keys,
        2
    );
}

#[test]
fn maximum_utf8_bounds_use_live_order_and_validation_precedes_empty_results() {
    let short = format!("{}a", "界".repeat(85));
    let long = format!("{}ab", "界".repeat(85));
    let maximum = "界".repeat(1024);
    let snapshot = initialized([short.clone(), long.clone(), maximum.clone()]);
    assert_eq!(
        selected(&snapshot, Some(&long), Some(&maximum), 100),
        vec![long.clone()]
    );
    assert_eq!(
        selected(&snapshot, Some(&maximum), None, 1),
        vec![maximum.clone()]
    );
    assert!(selected(&snapshot, Some(&maximum), Some(&short), 100).is_empty());
    assert!(
        snapshot
            .scan_text_range("t", Some(&"x".repeat(3073)), None, 0)
            .is_err()
    );
    assert!(
        snapshot
            .scan_text_range("t", None, None, MAX_ROWS + 1)
            .is_err()
    );
    assert!(snapshot.scan_text_range("missing", None, None, 0).is_err());
    assert!(snapshot.scan_integer_range("t", None, None, 0).is_err());
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(
        selected(&snapshot, None, None, 100),
        selected(&replay, None, None, 100)
    );
}

#[test]
fn historical_text_ranges_keep_old_images_through_loaded_tree_mutations() {
    let mut snapshot = initialized(["a".into(), "b".into(), format!("a{}", "x".repeat(3071))]);
    let old = snapshot.clone();
    let tree = snapshot.export_primary_tree("t").unwrap();
    snapshot.install_primary_tree("t", tree).unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Delete(Key::Text("a".into())),
        })
        .unwrap();
    assert_eq!(selected(&old, Some("a"), Some("b"), 1), vec!["a"]);
    assert_eq!(
        selected(&snapshot, Some("a"), Some("b"), 1),
        vec![format!("a{}", "x".repeat(3071))]
    );
    assert_eq!(selected(&snapshot, Some("b"), None, 100), vec!["b"]);
}

#[test]
fn actual_ten_thousand_row_text_capacity_preserves_boundary_and_maximum_keys_in_ranges() {
    let keys = (0..10_000)
        .map(|id| {
            if id == 9 {
                format!("{id:05}{}z", "界".repeat(1022))
            } else {
                format!("{id:05}{}", "x".repeat(if id % 7 == 0 { 252 } else { 251 }))
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(keys[9].len(), 3072);
    let snapshot = initialized(keys.iter().cloned());
    let before = snapshot.page_fingerprint();
    for (lower, upper) in [
        (None, None),
        (Some("00000"), Some("00015")),
        (Some("00013"), Some("09000")),
        (Some("09999"), None),
        (Some(keys[9].as_str()), Some(keys[100].as_str())),
    ] {
        for limit in [1, 13, 14, 200, MAX_ROWS] {
            let expected = keys
                .iter()
                .filter(|key| {
                    lower.is_none_or(|lower| key.as_str() >= lower)
                        && upper.is_none_or(|upper| key.as_str() < upper)
                })
                .take(limit)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(selected(&snapshot, lower, upper, limit), expected);
        }
    }
    let info = snapshot.primary_index_info("t").unwrap();
    assert_eq!(info.entries + info.excluded_long_keys, 10_000);
    assert_eq!(
        info.excluded_long_keys,
        keys.iter().filter(|key| key.len() > 256).count()
    );
    assert_eq!(snapshot.page_fingerprint(), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn ordered_text_intervals_match_an_independent_utf8_set(
        input in proptest::collection::btree_set("[a-z界😀\\x00]{0,12}", 0..50),
        lower in proptest::option::of("[a-z界😀\\x00]{0,12}"),
        upper in proptest::option::of("[a-z界😀\\x00]{0,12}"),
        limit in 0usize..60
    ) {
        let mut keys = input;
        keys.insert(format!("a{}", "界".repeat(1023)));
        keys.insert(format!("b{}", "z".repeat(256)));
        let snapshot = initialized(keys.iter().cloned());
        let expected = keys.iter().filter(|key| lower.as_ref().is_none_or(|lower| *key >= lower) && upper.as_ref().is_none_or(|upper| *key < upper)).take(limit).cloned().collect::<Vec<_>>();
        prop_assert_eq!(selected(&snapshot, lower.as_deref(), upper.as_deref(), limit), expected);
    }
}
