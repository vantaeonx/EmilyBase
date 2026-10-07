use super::*;
#[test]
fn empty_sources_and_boundary_indices_use_checked_logical_offsets() {
    let left = vec![Value::Integer(1)];
    let right = vec![Value::Text("я\0".into()), Value::Null];
    let view = RowView::joined(&left, &right);
    assert_eq!(view.get(0), left.first());
    assert_eq!(view.get(1), right.first());
    assert_eq!(view.get(2), right.get(1));
    assert!(view.get(3).is_none());
    assert!(view.get(usize::MAX).is_none());
    assert_eq!(view.to_owned(), [left.clone(), right.clone()].concat());
    let empty = Vec::new();
    assert_eq!(RowView::joined(&empty, &right).to_owned(), right);
    assert_eq!(RowView::joined(&left, &empty).to_owned(), left);
    assert_eq!(RowView::single(&empty).bytes().unwrap(), 24);
}

#[test]
fn every_split_preserves_logical_payload_cost_and_all_value_bits() {
    let row = vec![
        Value::Boolean(true),
        Value::Integer(i64::MIN),
        Value::Float(-0.0),
        Value::Text("я\0λ".into()),
        Value::Bytes(vec![0, 255]),
        Value::Null,
    ];
    for split in 0..=row.len() {
        let left = row[..split].to_vec();
        let right = row[split..].to_vec();
        let view = RowView::joined(&left, &right);
        assert_eq!(view.bytes().unwrap(), crate::execute::row_bytes(&row));
        assert_eq!(view.to_owned(), row);
        let Value::Float(value) = view.get(2).unwrap() else {
            panic!("generated float");
        };
        assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    }
}
#[test]
fn checked_sort_indices_and_shared_null_direction_rules_match_owned_rows() {
    let left = vec![Value::Integer(7), Value::Null];
    let right = vec![Value::Text("b".into())];
    let view = RowView::joined(&left, &right);
    let owned = view.to_owned();
    for index in 0..owned.len() {
        for descending in [false, true] {
            for nulls_first in [false, true] {
                let order = [SortKey {
                    index,
                    descending,
                    nulls_first,
                }];
                view.validate_order(&order).unwrap();
                let mut other = owned.clone();
                other[index] = Value::Null;
                assert_eq!(
                    view.compare(&other, &order).unwrap(),
                    crate::select::compare_rows(&owned, &other, &order)
                );
            }
        }
    }
    let bad = [SortKey {
        index: usize::MAX,
        descending: false,
        nulls_first: false,
    }];
    assert!(matches!(
        view.validate_order(&bad),
        Err(ExecutionError::Plan)
    ));
    assert!(matches!(
        view.compare(&owned, &bad),
        Err(ExecutionError::Plan)
    ));
}
