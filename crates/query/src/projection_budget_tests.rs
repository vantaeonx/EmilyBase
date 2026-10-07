use super::*;

fn columns(indices: &[usize]) -> Vec<(usize, String)> {
    indices.iter().map(|n| (*n, format!("c{n}"))).collect()
}
#[test]
fn selected_occurrences_charge_only_their_payload_and_preserve_float_bits() {
    let source = vec![
        Value::Text("я".repeat(1536)),
        Value::Null,
        Value::Bytes(vec![0, 1, 255]),
        Value::Float(-0.0),
        Value::Boolean(true),
    ];
    let mut budget = Budget { work: 0, output: 0 };
    let result = crate::projection::row(&columns(&[2, 1, 2, 3, 4]), &source, &mut budget).unwrap();
    assert_eq!(
        result,
        vec![
            source[2].clone(),
            Value::Null,
            source[2].clone(),
            Value::Float(-0.0),
            Value::Boolean(true)
        ]
    );
    assert_eq!(budget.output, 24 + 5 * 32 + 6);
    assert_eq!(budget.work, 0);
    let Value::Float(value) = result[3] else {
        panic!("float fixture")
    };
    assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    assert_eq!(source[0], Value::Text("я".repeat(1536)));
}
#[test]
fn shared_budget_is_admitted_before_returning_any_new_row() {
    let source = vec![Value::Bytes(vec![0; 3072])];
    let fields = columns(&vec![0; 32]);
    let charge = 24 + 32 * (32 + 3072);
    let mut budget = Budget {
        work: 7,
        output: MAX_OUTPUT_BYTES - charge,
    };
    let result = crate::projection::row(&fields, &source, &mut budget).unwrap();
    assert_eq!(result.len(), 32);
    assert_eq!(budget.output, MAX_OUTPUT_BYTES);
    assert!(matches!(
        crate::projection::row(&fields, &source, &mut budget),
        Err(ExecutionError::Limit("output bytes"))
    ));
    assert_eq!(budget.work, 7);
    assert_eq!(source[0], Value::Bytes(vec![0; 3072]));
}
#[test]
fn invalid_bound_column_refuses_before_charging_or_copying_source() {
    let source = vec![Value::Text("original".into())];
    let fields = columns(&[0, 1]);
    let mut budget = Budget {
        work: 9,
        output: 17,
    };
    assert!(matches!(
        crate::projection::row(&fields, &source, &mut budget),
        Err(ExecutionError::Plan)
    ));
    assert_eq!(budget.output, 17);
    assert_eq!(budget.work, 9);
    assert_eq!(source, [Value::Text("original".into())]);
}
#[test]
fn empty_text_bytes_and_null_still_have_the_original_cell_and_row_charge() {
    let source = vec![
        Value::Text(String::new()),
        Value::Bytes(Vec::new()),
        Value::Null,
    ];
    let mut budget = Budget { work: 0, output: 0 };
    let fields = columns(&[0, 1, 2, 2]);
    for _ in 0..3 {
        assert_eq!(
            crate::projection::row(&fields, &source, &mut budget)
                .unwrap()
                .len(),
            4
        );
    }
    assert_eq!(budget.output, 3 * (24 + 4 * 32));
}
