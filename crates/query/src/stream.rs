use crate::execute::{Budget, ResultSet, RunResult, row_bytes};
use crate::plan::Plan;
use emilybase_catalog::{Key, Row};
use emilybase_database::Snapshot;

fn retain(plan: &Plan, row: &Row, budget: &mut Budget, rows: &mut Vec<Row>) -> RunResult<()> {
    budget.step()?;
    if let Some(predicate) = &plan.filter
        && predicate.evaluate(row, budget)? != Some(true)
    {
        return Ok(());
    }
    let projected = plan
        .columns
        .iter()
        .map(|(index, _)| row[*index].clone())
        .collect::<Row>();
    budget.output(row_bytes(&projected))?;
    rows.push(projected);
    Ok(())
}

/// No join and either no requested order or a unique primary column first.
/// Binding has already validated all predicates, sort fields and parameters.
pub(crate) fn run(snapshot: &Snapshot, plan: &Plan, budget: &mut Budget) -> RunResult<ResultSet> {
    let mut rows = Vec::new();
    if let Some(key) = &plan.key {
        if let Some(row) = point(snapshot, &plan.table, key)? {
            retain(plan, row, budget, &mut rows)?;
        }
    } else if !plan.range.as_ref().is_some_and(|range| range.empty()) {
        let (lower, upper) = plan
            .range
            .as_ref()
            .map_or((None, None), |range| range.bounds());
        let mut cursor = snapshot.primary_rows(&plan.table, lower.as_ref(), upper.as_ref())?;
        while rows.len() < plan.limit {
            let item = if plan.primary_order == Some(true) {
                cursor.next_back()
            } else {
                cursor.next()
            };
            let Some(row) = item else {
                break;
            };
            retain(plan, row?, budget, &mut rows)?;
        }
    }
    Ok(ResultSet {
        columns: plan.columns.iter().map(|(_, name)| name.clone()).collect(),
        rows,
        affected: 0,
    })
}

pub(crate) fn point<'a>(
    snapshot: &'a Snapshot,
    table: &str,
    key: &Key,
) -> RunResult<Option<&'a Row>> {
    let Some(mut row) = snapshot.get(table, key)? else {
        return Ok(None);
    };
    // Long keys use the retained map in get; check the physical image here too.
    if matches!(key, Key::Text(text) if text.len() > emilybase_index::MAX_KEY_BYTES) {
        let location = snapshot
            .row_location(table, key)?
            .ok_or(emilybase_database::Error::StaleLocation)?;
        row = snapshot.resolve_row_location(table, key, location)?;
    }
    Ok(Some(row))
}
