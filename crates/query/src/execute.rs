use crate::ast::*;
use crate::plan::{Layout, Plan, primary_key};
use crate::predicate::{Predicate, bind};
use crate::{MAX_PARAMETERS, parse};
use emilybase_catalog::{Column, Row, Schema, Value};
use emilybase_transactions::{Database, Transaction};

#[cfg(all(test, target_os = "linux"))]
#[path = "matching_memory.rs"]
mod matching_memory;

pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_QUERY_WORK: usize = 100_000;
pub type RunResult<T> = std::result::Result<T, ExecutionError>;
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Syntax(#[from] crate::Error),
    #[error(transparent)]
    Database(#[from] emilybase_database::Error),
    #[error(transparent)]
    Transaction(#[from] emilybase_transactions::Error),
    #[error(transparent)]
    Catalog(#[from] emilybase_catalog::Error),
    #[error("unknown or ambiguous SQL column/qualifier")]
    Column,
    #[error("missing parameter {0}")]
    Binding(usize),
    #[error("SQL value type mismatch")]
    Type,
    #[error("SQL execution limit exceeded: {0}")]
    Limit(&'static str),
    #[error("invalid transaction-control placement")]
    Control,
    #[error("invalid resolved SQL plan")]
    Plan,
    #[error("duplicate columns or unsupported primary-key assignment")]
    Assignment,
}
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub affected: usize,
}
#[derive(Debug, serde::Serialize)]
pub struct Report {
    pub transaction: u64,
    pub committed: bool,
    pub results: Vec<ResultSet>,
}

/// Successful execution with an exclusively owned, still uncommitted transaction.
/// Dropping this value discards all staged writes, including those preceding SQL.
#[must_use = "staged SQL must be explicitly committed or discarded"]
pub struct StagedScript<'a> {
    transaction: Transaction<'a>,
    results: Vec<ResultSet>,
}

impl<'a> StagedScript<'a> {
    /// Inspect bounded query results without releasing the transaction owner.
    pub fn results(&self) -> &[ResultSet] {
        &self.results
    }

    /// Recover the successful transaction to add typed writes before one commit.
    /// Subsequent write failures retain Transaction's abort-on-error behavior.
    pub fn into_parts(self) -> (Transaction<'a>, Vec<ResultSet>) {
        (self.transaction, self.results)
    }
}
pub(crate) struct Budget {
    work: usize,
    output: usize,
}
impl Budget {
    pub(crate) fn step(&mut self) -> RunResult<()> {
        self.work += 1;
        if self.work > MAX_QUERY_WORK {
            Err(ExecutionError::Limit("query work"))
        } else {
            Ok(())
        }
    }
    pub(crate) fn output(&mut self, bytes: usize) -> RunResult<()> {
        self.output += bytes;
        if self.output > MAX_OUTPUT_BYTES {
            Err(ExecutionError::Limit("output bytes"))
        } else {
            Ok(())
        }
    }
}
pub(crate) fn value_bytes(value: &Value) -> usize {
    32 + match value {
        Value::Text(value) => value.len(),
        Value::Bytes(value) => value.len(),
        _ => 0,
    }
}
pub(crate) fn row_bytes(row: &Row) -> usize {
    24 + row.iter().map(value_bytes).sum::<usize>()
}

#[cfg(test)]
#[path = "projection_budget_tests.rs"]
mod projection_budget_tests;

#[cfg(test)]
#[path = "borrowed_candidate_work.rs"]
mod borrowed_candidate_work;

#[cfg(test)]
#[path = "single_sort_work.rs"]
mod single_sort_work;

pub(crate) fn validate_parameters(parameters: &[Value]) -> RunResult<()> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(ExecutionError::Limit("bindings"));
    }
    for value in parameters {
        value.validate()?;
    }
    Ok(())
}

/// Stage a bounded SQL script without committing it or accepting SQL controls.
///
/// Ownership is intentional: syntax, binding, planning and execution failures all
/// drop the entire transaction. A caller cannot catch an error and then commit
/// earlier staged writes. On success, use `into_parts` to append typed metadata
/// and explicitly commit once. Each call has its own work/output budgets; the
/// original transaction's event/page/WAL limits span all composed writes.
pub fn stage<'a>(
    transaction: Transaction<'a>,
    sql: &str,
    parameters: &[Value],
) -> RunResult<StagedScript<'a>> {
    validate_parameters(parameters)?;
    let statements = parse(sql)?;
    if statements.iter().any(is_control) {
        return Err(ExecutionError::Control);
    }
    stage_statements(transaction, statements, parameters)
}

fn is_control(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Begin | Statement::Commit | Statement::Rollback
    )
}

fn stage_statements<'a>(
    mut transaction: Transaction<'a>,
    statements: Vec<Statement>,
    parameters: &[Value],
) -> RunResult<StagedScript<'a>> {
    // Even an empty script must not return an already aborted transaction.
    transaction.view()?;
    let mut results = Vec::new();
    let mut budget = Budget { work: 0, output: 0 };
    for statement in statements {
        results.push(run(&mut transaction, statement, parameters, &mut budget)?);
    }
    Ok(StagedScript {
        transaction,
        results,
    })
}

/// Execute one whole script atomically. Any error drops all staged writes; ACK follows WAL sync.
pub fn execute(database: &mut Database, sql: &str, parameters: &[Value]) -> RunResult<Report> {
    validate_parameters(parameters)?;
    let mut statements = parse(sql)?;
    let mut rollback = false;
    if matches!(statements.first(), Some(Statement::Begin)) {
        statements.remove(0);
        rollback = match statements.pop() {
            Some(Statement::Commit) => false,
            Some(Statement::Rollback) => true,
            _ => return Err(ExecutionError::Control),
        };
    }
    if statements.iter().any(is_control) {
        return Err(ExecutionError::Control);
    }
    let previous = database.last_transaction();
    let (tx, results) = stage_statements(database.begin()?, statements, parameters)?.into_parts();
    let transaction = if rollback {
        tx.rollback();
        previous
    } else {
        tx.commit()?
    };
    Ok(Report {
        transaction,
        committed: !rollback,
        results,
    })
}

/// Evaluate exactly one SELECT on a validated snapshot, without locks, files or mutations.
/// The caller determines whether the snapshot is committed or detached staged state.
pub fn query(
    snapshot: &emilybase_database::Snapshot,
    sql: &str,
    parameters: &[Value],
) -> RunResult<ResultSet> {
    validate_parameters(parameters)?;
    let statements = parse(sql)?;
    let [Statement::Select(select)] = statements.as_slice() else {
        return Err(ExecutionError::Control);
    };
    let mut budget = Budget { work: 0, output: 0 };
    crate::select::run(
        snapshot,
        Plan::compile(snapshot, select, parameters)?,
        &mut budget,
    )
}

fn indices(schema: &Schema, names: &[String]) -> RunResult<Vec<usize>> {
    let mut used = std::collections::BTreeSet::new();
    let mut positions = Vec::new();
    for name in names {
        let position = schema
            .columns
            .iter()
            .position(|c| &c.name == name)
            .ok_or(ExecutionError::Column)?;
        if !used.insert(position) {
            return Err(ExecutionError::Assignment);
        }
        positions.push(position);
    }
    Ok(positions)
}
fn typed(value: &Value, column: &Column) -> RunResult<()> {
    if value == &Value::Null {
        if !column.nullable {
            return Err(ExecutionError::Type);
        }
    } else if value.data_type() != Some(column.data_type) {
        return Err(ExecutionError::Type);
    }
    Ok(())
}
fn matching<T>(
    tx: &Transaction<'_>,
    table: &str,
    filter: &Option<Expr>,
    parameters: &[Value],
    budget: &mut Budget,
    materialize: impl Fn(&Schema, &Row) -> RunResult<T>,
) -> RunResult<Vec<T>> {
    let snapshot = tx.view()?;
    let schema = snapshot.schema(table)?;
    let table_ref = TableRef {
        name: table.into(),
        alias: None,
    };
    let layout = Layout::new(snapshot, &[&table_ref])?;
    let filter = filter
        .as_ref()
        .map(|e| Predicate::compile(e, &layout, parameters))
        .transpose()?;
    let mut matching = Vec::new();
    let remaining = tx.remaining_events()?;
    let key = filter
        .as_ref()
        .and_then(|p| primary_key(p, usize::from(schema.primary_key)));
    let mut retain = |row: &Row| -> RunResult<()> {
        budget.step()?;
        if match &filter {
            None => true,
            Some(p) => p.evaluate(row, budget)? == Some(true),
        } {
            if matching.len() >= remaining {
                return Err(emilybase_transactions::Error::Limit.into());
            }
            matching.push(materialize(schema, row)?);
        }
        Ok(())
    };
    if let Some(key) = key {
        if let Some(row) = crate::stream::point(snapshot, table, &key)? {
            retain(row)?;
        }
    } else {
        let range = filter.as_ref().and_then(|predicate| {
            crate::range::primary_range(predicate, usize::from(schema.primary_key))
        });
        if !range.as_ref().is_some_and(|range| range.empty()) {
            let (lower, upper) = range.as_ref().map_or((None, None), |range| range.bounds());
            for row in snapshot.primary_rows(table, lower.as_ref(), upper.as_ref())? {
                retain(row?)?;
            }
        }
    }
    Ok(matching)
}
fn run(
    tx: &mut Transaction<'_>,
    statement: Statement,
    parameters: &[Value],
    budget: &mut Budget,
) -> RunResult<ResultSet> {
    let mut affected = 0;
    match statement {
        Statement::Create(schema) => {
            tx.create_table(schema)?;
        }
        Statement::Drop(table) => tx.drop_table(&table)?,
        Statement::Insert {
            table,
            columns,
            rows,
        } => {
            let schema = tx.view()?.schema(&table)?.clone();
            let positions = match columns {
                Some(names) => indices(&schema, &names)?,
                None => (0..schema.columns.len()).collect(),
            };
            for values in rows {
                if values.len() != positions.len() {
                    return Err(ExecutionError::Type);
                }
                let mut row = vec![Value::Null; schema.columns.len()];
                for (i, value) in positions.iter().zip(values) {
                    row[*i] = bind(&value, parameters)?;
                }
                tx.insert(&table, row)?;
                affected += 1;
            }
        }
        Statement::Select(select) => {
            return crate::select::run(
                tx.view()?,
                Plan::compile(tx.view()?, &select, parameters)?,
                budget,
            );
        }
        Statement::Update {
            table,
            assignments,
            filter,
        } => {
            let schema = tx.view()?.schema(&table)?.clone();
            let positions = indices(
                &schema,
                &assignments
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
            )?;
            let mut updates = Vec::new();
            for (position, (_, scalar)) in positions.into_iter().zip(assignments) {
                if position == usize::from(schema.primary_key) {
                    return Err(ExecutionError::Assignment);
                }
                let value = bind(&scalar, parameters)?;
                typed(&value, &schema.columns[position])?;
                updates.push((position, value));
            }
            for (key, mut row) in
                matching(tx, &table, &filter, parameters, budget, |schema, row| {
                    Ok((schema.key(row)?, row.clone()))
                })?
            {
                for (i, value) in &updates {
                    row[*i] = value.clone();
                }
                tx.update(&table, &key, row)?;
                affected += 1;
            }
        }
        Statement::Delete { table, filter } => {
            for key in matching(tx, &table, &filter, parameters, budget, |schema, row| {
                Ok(schema.key(row)?)
            })? {
                tx.delete(&table, &key)?;
                affected += 1;
            }
        }
        _ => return Err(ExecutionError::Control),
    }
    Ok(ResultSet {
        columns: Vec::new(),
        rows: Vec::new(),
        affected,
    })
}

#[cfg(test)]
#[path = "join_work.rs"]
mod join_work;

#[cfg(test)]
#[path = "nested_join_work.rs"]
mod nested_join_work;
