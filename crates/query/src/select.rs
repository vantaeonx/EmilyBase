use crate::MAX_RESULT_ROWS;
use crate::execute::{Budget, ExecutionError, MAX_OUTPUT_BYTES, ResultSet, RunResult, row_bytes};
use crate::plan::{Plan, SortKey};
use crate::predicate::value_order;
use emilybase_catalog::{Row, Value};
use emilybase_database::{MAX_ROWS, Snapshot};
use std::cmp::Ordering;

pub(crate) fn run(snapshot: &Snapshot, plan: Plan, budget: &mut Budget) -> RunResult<ResultSet> {
    if plan.limit == 0 {
        return Ok(ResultSet {
            columns: plan.columns.into_iter().map(|(_, name)| name).collect(),
            rows: Vec::new(),
            affected: 0,
        });
    }
    if plan.join.is_none() && (plan.order.is_empty() || plan.primary_order.is_some()) {
        return crate::stream::run(snapshot, &plan, budget);
    }
    if plan.primary_join.is_some() {
        return crate::primary_join::run(snapshot, plan, budget);
    }
    let source = match &plan.key {
        Some(key) => snapshot
            .get(&plan.table, key)?
            .cloned()
            .into_iter()
            .collect(),
        None => match &plan.range {
            Some(range) if range.empty() => Vec::new(),
            Some(range) => range.scan(snapshot, &plan.table, MAX_ROWS)?,
            None => snapshot.scan(&plan.table, MAX_ROWS)?,
        },
    };
    let joined = plan
        .join
        .as_ref()
        .map(|(t, _)| snapshot.scan(t, MAX_ROWS))
        .transpose()?;
    let mut retained = Vec::new();
    let mut bytes = 0;
    'scan: for left in source {
        let count = joined.as_ref().map_or(1, Vec::len);
        for position in 0..count {
            budget.step()?;
            let mut row = left.clone();
            if let Some(right) = &joined {
                row.extend(right[position].clone());
            }
            if let Some((_, predicate)) = &plan.join
                && predicate.evaluate(&row, budget)? != Some(true)
            {
                continue;
            }
            if let Some(predicate) = &plan.filter
                && predicate.evaluate(&row, budget)? != Some(true)
            {
                continue;
            }
            bytes += row_bytes(&row);
            if retained.len() >= MAX_RESULT_ROWS || bytes > MAX_OUTPUT_BYTES {
                return Err(ExecutionError::Limit("intermediate rows/bytes"));
            }
            retained.push(row);
            if plan.order.is_empty() && retained.len() >= plan.limit {
                break 'scan;
            }
        }
    }
    finish(plan, retained, budget)
}

pub(crate) fn finish(
    plan: Plan,
    mut retained: Vec<Row>,
    budget: &mut Budget,
) -> RunResult<ResultSet> {
    // All sort columns resolve to schema-validated types before scanning, including empty input.
    retained.sort_by(|a, b| compare_rows(a, b, &plan.order));
    project(plan, retained, budget)
}

/// Rows arrive in the final order, either from a stable sort or bounded heap.
pub(crate) fn project(plan: Plan, retained: Vec<Row>, budget: &mut Budget) -> RunResult<ResultSet> {
    let mut rows = Vec::new();
    for row in retained.into_iter().take(plan.limit) {
        rows.push(crate::projection::row(&plan.columns, &row, budget)?);
    }
    Ok(ResultSet {
        columns: plan.columns.into_iter().map(|(_, name)| name).collect(),
        rows,
        affected: 0,
    })
}

/// Shared null/direction/type ordering for stable full sort and bounded selection.
/// Schema binding precedes either path, including empty input and LIMIT 0.
pub(crate) fn compare_rows(a: &Row, b: &Row, order: &[SortKey]) -> Ordering {
    for key in order {
        let (a, b) = (&a[key.index], &b[key.index]);
        let order = match (a, b) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => {
                if key.nulls_first {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (_, Value::Null) => {
                if key.nulls_first {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            _ => {
                let order = value_order(a, b).unwrap_or(Ordering::Equal);
                if key.descending {
                    order.reverse()
                } else {
                    order
                }
            }
        };
        if !order.is_eq() {
            return order;
        }
    }
    Ordering::Equal
}
