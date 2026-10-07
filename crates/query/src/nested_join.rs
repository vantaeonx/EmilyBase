//! Original nested-loop fallback, borrowing checked sources and rejected pairs.
use crate::MAX_RESULT_ROWS;
use crate::execute::{Budget, ExecutionError, MAX_OUTPUT_BYTES, ResultSet, RunResult};
use crate::plan::Plan;
use crate::row_view::RowView;
use emilybase_catalog::Row;
use emilybase_database::Snapshot;

fn left_rows<'a>(snapshot: &'a Snapshot, plan: &Plan) -> RunResult<Vec<&'a Row>> {
    if let Some(key) = &plan.key {
        return Ok(crate::stream::point(snapshot, &plan.table, key)?
            .into_iter()
            .collect());
    }
    if plan.range.as_ref().is_some_and(|range| range.empty()) {
        return Ok(Vec::new());
    }
    let (lower, upper) = plan
        .range
        .as_ref()
        .map_or((None, None), |range| range.bounds());
    Ok(snapshot
        .primary_rows(&plan.table, lower.as_ref(), upper.as_ref())?
        .collect::<emilybase_database::Result<Vec<_>>>()?)
}

pub(crate) fn run(snapshot: &Snapshot, plan: Plan, budget: &mut Budget) -> RunResult<ResultSet> {
    let (table, predicate) = plan.join.as_ref().ok_or(ExecutionError::Plan)?;
    if plan.primary_join.is_some() {
        return Err(ExecutionError::Plan);
    }
    // The global live-row bound also bounds these pointer vectors. Physical
    // images are checked once per source, before evaluating any pair. No source
    // row, string, byte payload or combined rejected row is copied here.
    let source = left_rows(snapshot, &plan)?;
    let joined = snapshot
        .primary_rows(table, None, None)?
        .collect::<emilybase_database::Result<Vec<_>>>()?;
    let mut retained = Vec::new();
    let mut bytes = 0usize;
    'scan: for left in source {
        for right in &joined {
            budget.step()?;
            let row = RowView::joined(left, right);
            if predicate.evaluate_view(row, budget)? != Some(true) {
                continue;
            }
            if let Some(filter) = &plan.filter
                && filter.evaluate_view(row, budget)? != Some(true)
            {
                continue;
            }
            bytes = bytes
                .checked_add(row.bytes()?)
                .ok_or(ExecutionError::Limit("intermediate rows/bytes"))?;
            if retained.len() >= MAX_RESULT_ROWS || bytes > MAX_OUTPUT_BYTES {
                return Err(ExecutionError::Limit("intermediate rows/bytes"));
            }
            // Keep the original full matched-row bound, stable full sort and
            // projection timing. Admission precedes the first candidate copy.
            retained.push(row.to_owned());
            if plan.order.is_empty() && retained.len() >= plan.limit {
                break 'scan;
            }
        }
    }
    crate::select::finish(plan, retained, budget)
}
