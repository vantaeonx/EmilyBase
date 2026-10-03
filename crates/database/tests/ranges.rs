use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Error, Event, EventKind, MAX_ROWS, Snapshot};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn initialized(keys: impl IntoIterator<Item = i64>) -> Snapshot {
    let mut snapshot = Snapshot::empty().unwrap();
    snapshot
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "t".into(),
                columns: vec![Column {
                    name: "id".into(),
                    data_type: DataType::Integer,
                    nullable: false,
                }],
                primary_key: 0,
            }),
        })
        .unwrap();
    for key in keys {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(key)]),
            })
            .unwrap();
    }
    snapshot
}
fn keys(snapshot: &Snapshot, lower: Option<i64>, upper: Option<i64>, limit: usize) -> Vec<i64> {
    snapshot
        .scan_integer_range("t", lower, upper, limit)
        .unwrap()
        .into_iter()
        .map(|row| match row[0] {
            Value::Integer(n) => n,
            _ => panic!("wrong row type"),
        })
        .collect()
}

#[test]
fn integer_ranges_cross_linked_leaves_and_respect_all_endpoint_and_limit_boundaries() {
    let snapshot = initialized([i64::MIN, -100, -1, 0, 1, 13, 14, 15, 99, 100, i64::MAX]);
    let before = snapshot
        .pages()
        .map(|page| page.encode())
        .collect::<Vec<_>>();
    let cases = [
        (
            None,
            None,
            100,
            vec![i64::MIN, -100, -1, 0, 1, 13, 14, 15, 99, 100, i64::MAX],
        ),
        (Some(0), Some(15), 100, vec![0, 1, 13, 14]),
        (Some(i64::MIN), Some(-100), 100, vec![i64::MIN]),
        (Some(i64::MAX), None, 100, vec![i64::MAX]),
        (None, Some(i64::MIN), 100, vec![]),
        (Some(15), Some(15), 100, vec![]),
        (Some(20), Some(10), 100, vec![]),
        (Some(-1), None, 2, vec![-1, 0]),
        (None, None, 0, vec![]),
    ];
    for (lower, upper, limit, expected) in cases {
        assert_eq!(keys(&snapshot, lower, upper, limit), expected);
    }
    let large = initialized(0..300);
    assert_eq!(
        keys(&large, Some(11), Some(285), MAX_ROWS),
        (11..285).collect::<Vec<_>>()
    );
    assert_eq!(
        keys(&large, Some(13), Some(200), 14),
        (13..27).collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot
            .pages()
            .map(|page| page.encode())
            .collect::<Vec<_>>(),
        before
    );
    assert!(matches!(
        snapshot.scan_integer_range("t", None, None, MAX_ROWS + 1),
        Err(Error::Limit(_))
    ));
    assert!(matches!(
        snapshot.scan_integer_range("missing", None, None, 0),
        Err(Error::NoTable)
    ));
}

#[test]
fn ranges_keep_old_snapshots_and_replayed_mutations_consistent() {
    let mut snapshot = initialized(0..64);
    assert_eq!(
        keys(&snapshot, Some(10), Some(30), MAX_ROWS),
        (10..30).collect::<Vec<_>>()
    );
    let historical = snapshot.clone();
    for key in 10..30 {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Integer(key)),
            })
            .unwrap();
    }
    for key in 64..80 {
        snapshot
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(key)]),
            })
            .unwrap();
    }
    assert_eq!(
        keys(&historical, Some(10), Some(30), MAX_ROWS),
        (10..30).collect::<Vec<_>>()
    );
    assert!(keys(&snapshot, Some(10), Some(30), MAX_ROWS).is_empty());
    let replay = Snapshot::from_pages(snapshot.pages().cloned().collect()).unwrap();
    assert_eq!(
        keys(&snapshot, Some(25), None, MAX_ROWS),
        keys(&replay, Some(25), None, MAX_ROWS)
    );
    assert_eq!(
        keys(&snapshot, Some(60), Some(70), MAX_ROWS),
        (60..70).collect::<Vec<_>>()
    );
}

#[test]
fn integer_range_rejects_text_schema_even_for_empty_bounds_and_zero_limit() {
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
    for (lower, upper, limit) in [(None, None, 0), (Some(9), Some(1), 1)] {
        assert!(matches!(
            snapshot.scan_integer_range("t", lower, upper, limit),
            Err(Error::IntegerRangeType)
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn generated_integer_intervals_match_independent_ordered_keys_and_preserve_page_bytes(
        input in proptest::collection::vec(any::<i64>(),0..100),
        lower in proptest::option::of(any::<i64>()),upper in proptest::option::of(any::<i64>()),limit in 0usize..120
    ) {
        let model=input.into_iter().collect::<BTreeSet<_>>();
        let snapshot=initialized(model.iter().copied());
        let before=snapshot.pages().map(|page|page.encode()).collect::<Vec<_>>();
        let expected=model.iter().filter(|key|lower.is_none_or(|n|**key>=n) && upper.is_none_or(|n|**key<n)).take(limit).copied().collect::<Vec<_>>();
        prop_assert_eq!(keys(&snapshot,lower,upper,limit),expected);
        prop_assert_eq!(snapshot.pages().map(|page|page.encode()).collect::<Vec<_>>(),before);
    }
}
