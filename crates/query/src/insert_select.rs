use crate::ast::Select;
use crate::execute::{Budget, ExecutionError, RunResult, indices};
use crate::plan::{Layout, Plan};
use emilybase_catalog::Value;
use emilybase_transactions::Transaction;

pub(crate) fn run(
    transaction: &mut Transaction<'_>,
    table: &str,
    columns: Option<&[String]>,
    select: &Select,
    parameters: &[Value],
    budget: &mut Budget,
) -> RunResult<usize> {
    let snapshot = transaction.view()?;
    let schema = snapshot.schema(table)?.clone();
    let positions = match columns {
        Some(names) => indices(&schema, names)?,
        None => (0..schema.columns.len()).collect(),
    };
    let mut plan = Plan::compile(snapshot, select, parameters)?;
    if positions.len() != plan.columns.len() {
        return Err(ExecutionError::Type);
    }
    let mut tables = vec![&select.from];
    if let Some((source, _)) = &select.join {
        tables.push(source);
    }
    let layout = Layout::new(snapshot, &tables)?;
    for (target, (source, _)) in positions.iter().zip(&plan.columns) {
        let target = schema.columns.get(*target).ok_or(ExecutionError::Plan)?;
        let source = layout.columns.get(*source).ok_or(ExecutionError::Plan)?;
        if target.data_type != source.data_type {
            return Err(ExecutionError::Type);
        }
    }
    let remaining = transaction.remaining_events()?;
    // One lookahead proves overflow without owning an unbounded result set.
    // Select order/filter/user LIMIT semantics are unchanged for every success.
    plan.limit = plan.limit.min(remaining + 1);
    let selected = crate::select::run(snapshot, plan, budget)?;
    if selected.rows.len() > remaining {
        return Err(emilybase_transactions::Error::Limit.into());
    }
    let affected = selected.rows.len();
    // Finish the source read before mutating: self-copy cannot see its own inserts.
    for values in selected.rows {
        budget.step()?;
        let mut row = vec![Value::Null; schema.columns.len()];
        for (position, value) in positions.iter().zip(values) {
            row[*position] = value;
        }
        transaction.insert(table, row)?;
    }
    Ok(affected)
}
