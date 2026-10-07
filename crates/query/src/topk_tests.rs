use super::*;
use emilybase_catalog::Value;

fn order(descending: bool, nulls_first: bool) -> Vec<SortKey> {
    vec![SortKey {
        index: 0,
        descending,
        nulls_first,
    }]
}
#[test]
fn worst_root_is_replaced_and_discarded_ties_keep_original_input() {
    let order = order(false, false);
    let mut top = TopK::new(&order, 2);
    for (key, id) in [(9, 0), (4, 1), (4, 2), (8, 3), (1, 4), (4, 5)] {
        top.push(vec![Value::Integer(key), Value::Integer(id)])
            .unwrap();
        assert!(top.heap.len() <= 2);
        assert_eq!(top.bytes, top.heap.iter().map(|e| row_bytes(&e.row)).sum());
    }
    assert_eq!(top.seen, 6);
    assert_eq!(
        top.into_rows(),
        [
            vec![Value::Integer(1), Value::Integer(4)],
            vec![Value::Integer(4), Value::Integer(1)]
        ]
    );
}
#[test]
fn reverse_and_null_rules_agree_with_stable_full_sort_for_every_prefix() {
    for descending in [false, true] {
        for nulls_first in [false, true] {
            let order = order(descending, nulls_first);
            let rows = [
                Value::Null,
                Value::Integer(4),
                Value::Integer(-2),
                Value::Null,
                Value::Integer(4),
            ]
            .into_iter()
            .enumerate()
            .map(|(n, v)| vec![v, Value::Integer(n as i64)])
            .collect::<Vec<_>>();
            let mut full = rows.clone();
            full.sort_by(|a, b| crate::select::compare_rows(a, b, &order));
            for limit in 0..=rows.len() + 1 {
                let mut top = TopK::new(&order, limit);
                for row in &rows {
                    top.push(row.clone()).unwrap();
                }
                assert_eq!(
                    top.into_rows(),
                    full.iter().take(limit).cloned().collect::<Vec<_>>()
                );
            }
        }
    }
}
#[test]
fn discarded_candidates_still_exhaust_original_match_count() {
    let order = order(false, false);
    let mut top = TopK::new(&order, 1);
    for n in 0..MAX_RESULT_ROWS {
        top.push(vec![Value::Integer(n as i64)]).unwrap();
    }
    assert_eq!(top.heap.len(), 1);
    assert!(matches!(
        top.push(vec![Value::Integer(-1)]),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert_eq!(top.seen, MAX_RESULT_ROWS);
    assert_eq!(top.into_rows(), [vec![Value::Integer(0)]]);
}
#[test]
fn refused_growth_preserves_heap_and_actual_retained_byte_counter() {
    let order = order(false, false);
    let mut top = TopK::new(&order, MAX_RESULT_ROWS);
    loop {
        let row = vec![Value::Text("x".repeat(3072)); 16];
        let before = top.bytes;
        let count = top.heap.len();
        if top.push(row).is_err() {
            assert_eq!(top.bytes, before);
            assert_eq!(top.heap.len(), count);
            break;
        }
        assert!(top.bytes <= MAX_OUTPUT_BYTES);
    }
    assert_eq!(top.bytes, top.heap.iter().map(|e| row_bytes(&e.row)).sum());
}
#[test]
fn larger_better_replacement_is_checked_before_changing_the_selected_set() {
    let order = order(false, false);
    let mut top = TopK::new(&order, 1);
    top.push(vec![Value::Integer(1)]).unwrap();
    let before = top.bytes;
    let huge = vec![Value::Integer(0), Value::Bytes(vec![0; MAX_OUTPUT_BYTES])];
    assert!(matches!(
        top.push(huge),
        Err(ExecutionError::Limit("intermediate rows/bytes"))
    ));
    assert_eq!(top.bytes, before);
    assert_eq!(top.into_rows(), [vec![Value::Integer(1)]]);
}

#[test]
fn dropping_a_large_worse_candidate_does_not_charge_retained_bytes() {
    let order = order(false, false);
    let mut top = TopK::new(&order, 1);
    top.push(vec![Value::Integer(0)]).unwrap();
    let before = top.bytes;
    top.push(vec![
        Value::Integer(1),
        Value::Bytes(vec![0; MAX_OUTPUT_BYTES]),
    ])
    .unwrap();
    assert_eq!(top.bytes, before);
    assert_eq!(top.heap.len(), 1);
    assert_eq!(top.into_rows(), [vec![Value::Integer(0)]]);
}
#[test]
fn smaller_replacement_releases_old_payload_before_the_next_match() {
    let order = order(false, false);
    let mut top = TopK::new(&order, 1);
    top.push(vec![Value::Integer(9), Value::Text("x".repeat(3072))])
        .unwrap();
    let replacement = vec![Value::Integer(1), Value::Text("a".into())];
    let bytes = row_bytes(&replacement);
    top.push(replacement.clone()).unwrap();
    assert_eq!(top.bytes, bytes);
    assert_eq!(top.into_rows(), [replacement]);
}
