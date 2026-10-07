//! Probe a unique right primary key without materializing either source table.
use crate::MAX_RESULT_ROWS;
use crate::ast::Compare;
use crate::execute::{Budget, ExecutionError, MAX_OUTPUT_BYTES, ResultSet, RunResult, row_bytes};
use crate::plan::Plan;
use crate::predicate::{BoundOperand, Predicate};
use emilybase_catalog::{Key, Value};
use emilybase_database::Snapshot;

/// Only a necessary equality under AND can eliminate other right candidates.
/// OR/NOT, same-side comparisons and non-primary keys keep the original plan.
/// The complete bound predicate still executes on every selected candidate.
pub(crate) fn lookup_column(
    predicate: &Predicate,
    left_width: usize,
    right_primary: usize,
) -> Option<usize> {
    match predicate {
        Predicate::Compare(BoundOperand::Column(a), Compare::Eq, BoundOperand::Column(b)) => {
            if *a < left_width && *b == right_primary {
                Some(*a)
            } else if *b < left_width && *a == right_primary {
                Some(*b)
            } else {
                None
            }
        }
        Predicate::And(a, b) => lookup_column(a, left_width, right_primary)
            .or_else(|| lookup_column(b, left_width, right_primary)),
        _ => None,
    }
}

pub(crate) fn run(snapshot: &Snapshot, plan: Plan, budget: &mut Budget) -> RunResult<ResultSet> {
    let left_column = plan.primary_join.ok_or(ExecutionError::Plan)?;
    let (right_table, predicate) = plan.join.as_ref().ok_or(ExecutionError::Plan)?;
    let mut retained = Vec::new();
    let mut bytes = 0;
    for left in snapshot.primary_rows(&plan.table, None, None)? {
        // Missing/null probes also consume work; LIMIT cannot hide endless misses.
        budget.step()?;
        let left = left?;
        let value = left.get(left_column).ok_or(ExecutionError::Plan)?;
        if matches!(value, Value::Null) {
            continue;
        }
        let key = Key::from_value(value)?;
        // Short keys use the original B+ tree; long keys keep the validated map
        // path and physical row-location check used by ordinary point SELECT.
        let Some(right) = crate::stream::point(snapshot, right_table, &key)? else {
            continue;
        };
        budget.step()?;
        let mut row = left.clone();
        row.extend(right.iter().cloned());
        if predicate.evaluate(&row, budget)? != Some(true) {
            continue;
        }
        if let Some(filter) = &plan.filter
            && filter.evaluate(&row, budget)? != Some(true)
        {
            continue;
        }
        bytes += row_bytes(&row);
        if retained.len() >= MAX_RESULT_ROWS || bytes > MAX_OUTPUT_BYTES {
            return Err(ExecutionError::Limit("intermediate rows/bytes"));
        }
        retained.push(row);
        if plan.order.is_empty() && retained.len() >= plan.limit {
            break;
        }
    }
    crate::select::finish(plan, retained, budget)
}
