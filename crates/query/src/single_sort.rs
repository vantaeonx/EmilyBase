//! Borrow checked source rows and retain only the best LIMIT full candidates.
use crate::execute::{Budget, ExecutionError, ResultSet, RunResult};
use crate::plan::Plan;
use crate::topk::TopK;
use emilybase_catalog::Row;
use emilybase_database::Snapshot;

fn retain(plan: &Plan, row: &Row, budget: &mut Budget, selected: &mut TopK<'_>) -> RunResult<()> {
    budget.step()?;
    if let Some(predicate) = &plan.filter
        && predicate.evaluate(row, budget)? != Some(true)
    {
        return Ok(());
    }
    selected.push(row.clone())
}

pub(crate) fn run(snapshot: &Snapshot, plan: Plan, budget: &mut Budget) -> RunResult<ResultSet> {
    if plan.join.is_some() || plan.primary_order.is_some() || plan.order.is_empty() {
        return Err(ExecutionError::Plan);
    }
    if plan.range.as_ref().is_some_and(|range| range.empty()) {
        return crate::select::project(plan, Vec::new(), budget);
    }
    let mut selected = TopK::new(&plan.order, plan.limit);
    if let Some(key) = &plan.key {
        if let Some(row) = crate::stream::point(snapshot, &plan.table, key)? {
            retain(&plan, row, budget, &mut selected)?;
        }
    } else {
        let (lower, upper) = plan
            .range
            .as_ref()
            .map_or((None, None), |range| range.bounds());
        // Other orderings cannot stop after LIMIT source visits. Necessary
        // primary bounds restrict access; complete WHERE still executes.
        for row in snapshot.primary_rows(&plan.table, lower.as_ref(), upper.as_ref())? {
            retain(&plan, row?, budget, &mut selected)?;
        }
    }
    let rows = selected.into_rows();
    crate::select::project(plan, rows, budget)
}
