# Owned staged SQL

The synchronous query library provides `stage(transaction, sql, parameters)` for
trusted Rust callers that must combine typed writes and SQL into one original
WAL commit. SQL itself cannot begin/commit/rollback this transaction. Successful
staging returns `StagedScript`, whose `results()` are bounded query results and
whose consuming `into_parts()` releases the still uncommitted transaction/results.

```rust,ignore
let mut transaction = database.begin()?;
transaction.insert("receipts", prefix)?;
let staged = emilybase_query::stage(transaction, script, parameters)?;
let (mut transaction, results) = staged.into_parts();
transaction.insert("receipts", suffix)?;
let acknowledged_transaction = transaction.commit()?;
```

`stage` consumes ownership before validating parameters, parsing, checking controls,
resolving schemas or running the script. Every error drops the entire transaction,
including caller writes that preceded the SQL. This prevents catching a planning
error and accidentally committing an earlier partial script. Dropping a successful
`StagedScript`, dropping its extracted transaction or calling rollback also discards
all changes. Failed subsequent typed writes abort the original transaction; commit
then refuses. No staging operation syncs or publishes WAL. An explicit successful
commit still acknowledges only after the original WAL durability boundary.

The same internal runner implements ordinary `execute`; its existing explicit
BEGIN/COMMIT or BEGIN/ROLLBACK wrapper and result/report behavior are preserved.
Empty SQL remains a syntax error. Reads use staged state, including tables created
by earlier typed/SQL writes. Existing shared historical snapshots remain unchanged.

SQL byte/token/statement/parameter bounds and one shared work/output budget apply
to each call. The original256-event transaction bound, page and WAL limits apply
to the complete composed transaction, including typed prefix/suffix writes and
multiple stage calls. Work/output budgets restart on another call; this library
API is not a whole-process or arbitrarily repeated-script resource reservation.

This is a foundation for verified migrations, not a migration receipt format,
version-order policy, schema-diff engine, HTTP transaction endpoint or public-user
authorization. The caller owns the exclusive mutable database borrow until it
commits/discards; do not hold it across uncontrolled network input.
