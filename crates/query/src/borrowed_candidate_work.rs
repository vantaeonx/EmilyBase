use super::*;
use crate::predicate::{BoundOperand, Predicate};
use crate::row_view::RowView;
fn col(index: usize) -> BoundOperand {
    BoundOperand::Column(index)
}
#[test]
fn joined_truth_comparison_and_null_nodes_preserve_three_valued_work() {
    let left = vec![Value::Integer(1), Value::Boolean(false)];
    let right = vec![Value::Integer(1), Value::Null];
    let borrowed = RowView::joined(&left, &right);
    let full = borrowed.to_owned();
    let predicates = [
        Predicate::Compare(col(0), Compare::Eq, col(2)),
        Predicate::And(
            Box::new(Predicate::Truth(col(1))),
            Box::new(Predicate::Truth(col(3))),
        ),
        Predicate::Or(
            Box::new(Predicate::Truth(col(1))),
            Box::new(Predicate::Truth(col(3))),
        ),
        Predicate::Not(Box::new(Predicate::Truth(col(3)))),
        Predicate::IsNull(col(3), false),
    ];
    for (predicate, expected, work) in predicates
        .into_iter()
        .zip([Some(true), Some(false), None, None, Some(true)])
        .zip([1, 3, 3, 2, 1])
        .map(|((a, b), c)| (a, b, c))
    {
        let mut a = Budget { work: 0, output: 0 };
        let mut b = Budget { work: 0, output: 0 };
        assert_eq!(predicate.evaluate_view(borrowed, &mut a).unwrap(), expected);
        assert_eq!(predicate.evaluate(&full, &mut b).unwrap(), expected);
        assert_eq!((a.work, b.work), (work, work));
        assert_eq!((a.output, b.output), (0, 0));
    }
    let invalid = Predicate::Truth(col(usize::MAX));
    assert!(matches!(
        invalid.evaluate_view(borrowed, &mut Budget { work: 0, output: 0 }),
        Err(ExecutionError::Plan)
    ));
}
#[test]
fn both_boolean_branches_still_execute_and_can_exhaust_shared_work() {
    let empty = Vec::new();
    let borrowed = RowView::single(&empty);
    let predicate = Predicate::And(
        Box::new(Predicate::Truth(BoundOperand::Value(Value::Boolean(false)))),
        Box::new(Predicate::Truth(BoundOperand::Value(Value::Boolean(true)))),
    );
    let mut budget = Budget {
        work: MAX_QUERY_WORK - 2,
        output: 17,
    };
    assert!(matches!(
        predicate.evaluate_view(borrowed, &mut budget),
        Err(ExecutionError::Limit("query work"))
    ));
    assert_eq!(budget.work, MAX_QUERY_WORK + 1);
    assert_eq!(budget.output, 17);
}
#[test]
fn joined_projection_admits_repeated_payload_and_checks_boundary_before_cloning() {
    let left = vec![Value::Integer(9)];
    let right = vec![Value::Text("я\0".into()), Value::Null];
    let view = RowView::joined(&left, &right);
    let columns = vec![
        (1, "a".into()),
        (0, "b".into()),
        (1, "c".into()),
        (2, "n".into()),
    ];
    let charge = 24 + 4 * 32 + 6;
    let mut budget = Budget {
        work: 7,
        output: MAX_OUTPUT_BYTES - charge,
    };
    assert_eq!(
        crate::projection::view(&columns, view, &mut budget).unwrap(),
        [
            right[0].clone(),
            left[0].clone(),
            right[0].clone(),
            Value::Null
        ]
    );
    assert_eq!((budget.work, budget.output), (7, MAX_OUTPUT_BYTES));
    assert!(matches!(
        crate::projection::view(&columns, view, &mut budget),
        Err(ExecutionError::Limit("output bytes"))
    ));
    let mut budget = Budget {
        work: 7,
        output: 31,
    };
    assert!(matches!(
        crate::projection::view(&[(3, "bad".into())], view, &mut budget),
        Err(ExecutionError::Plan)
    ));
    assert_eq!((budget.work, budget.output), (7, 31));
}
