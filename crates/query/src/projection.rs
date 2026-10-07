//! Admit selected payload before cloning; every result row shares one script budget.
use crate::execute::{Budget, ExecutionError, RunResult, value_bytes};
use emilybase_catalog::Row;

pub(crate) fn row(
    columns: &[(usize, String)],
    source: &Row,
    budget: &mut Budget,
) -> RunResult<Row> {
    // Count every selected occurrence, including repeated fields and NULLs.
    // Binding normally guarantees indices; malformed internal plans still refuse.
    let bytes = columns.iter().try_fold(24usize, |bytes, (index, _)| {
        let value = source.get(*index).ok_or(ExecutionError::Plan)?;
        bytes
            .checked_add(value_bytes(value))
            .ok_or(ExecutionError::Limit("output bytes"))
    })?;
    budget.output(bytes)?;
    Ok(columns
        .iter()
        .map(|(index, _)| source[*index].clone())
        .collect())
}
