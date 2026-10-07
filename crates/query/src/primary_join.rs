//! Probe a unique right primary key without materializing either source table.
use crate::MAX_RESULT_ROWS;
use crate::ast::Compare;
use crate::execute::{Budget, ExecutionError, MAX_OUTPUT_BYTES, ResultSet, RunResult, row_bytes};
use crate::plan::Plan;
use crate::predicate::{BoundOperand, Predicate};
use emilybase_catalog::{Key, Row, Value};
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
    // Right uniqueness preserves the left primary order. With no sort needed,
    // retain only projected output, rather than all hidden joined payloads.
    let streaming = plan.order.is_empty() || plan.primary_order.is_some();
    let mut retained = Vec::new();
    let mut bytes = 0;
    if plan.range.as_ref().is_some_and(|range| range.empty()) {
        return crate::select::finish(plan, retained, budget);
    }
    let mut point = plan
        .key
        .as_ref()
        .map(|key| crate::stream::point(snapshot, &plan.table, key))
        .transpose()?
        .flatten();
    let mut cursor = if plan.key.is_none() {
        let (lower, upper) = plan
            .range
            .as_ref()
            .map_or((None, None), |range| range.bounds());
        Some(snapshot.primary_rows(&plan.table, lower.as_ref(), upper.as_ref())?)
    } else {
        None
    };
    loop {
        let left = if let Some(cursor) = &mut cursor {
            if plan.primary_order == Some(true) {
                cursor.next_back()
            } else {
                cursor.next()
            }
        } else {
            point.take().map(Ok)
        };
        let Some(left) = left else {
            break;
        };
        // Missing/null probes also consume work; LIMIT counts accepted matches.
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
        if retained.len() >= MAX_RESULT_ROWS {
            return Err(ExecutionError::Limit("intermediate rows/bytes"));
        }
        if streaming {
            let projected = plan
                .columns
                .iter()
                .map(|(index, _)| row[*index].clone())
                .collect::<Row>();
            budget.output(row_bytes(&projected))?;
            retained.push(projected);
        } else {
            bytes += row_bytes(&row);
            if bytes > MAX_OUTPUT_BYTES {
                return Err(ExecutionError::Limit("intermediate rows/bytes"));
            }
            retained.push(row);
        }
        if streaming && retained.len() >= plan.limit {
            break;
        }
    }
    if streaming {
        Ok(ResultSet {
            columns: plan.columns.into_iter().map(|(_, name)| name).collect(),
            rows: retained,
            affected: 0,
        })
    } else {
        crate::select::finish(plan, retained, budget)
    }
}
