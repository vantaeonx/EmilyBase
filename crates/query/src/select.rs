use crate::execute::{Budget, ResultSet, RunResult};
use crate::plan::{Plan, SortKey};
use crate::predicate::value_order;
use emilybase_catalog::{Row, Value};
use emilybase_database::Snapshot;
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
    if plan.join.is_none() {
        return crate::single_sort::run(snapshot, plan, budget);
    }
    if plan.primary_join.is_some() {
        return crate::primary_join::run(snapshot, plan, budget);
    }
    crate::nested_join::run(snapshot, plan, budget)
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
        let order = compare_values(a, b, key);
        if !order.is_eq() {
            return order;
        }
    }
    Ordering::Equal
}

/// One comparator serves checked borrowed views and owned heap entries.
pub(crate) fn compare_values(a: &Value, b: &Value, key: &SortKey) -> Ordering {
    match (a, b) {
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
    }
}
