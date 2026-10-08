# ADR0095: Consume transaction ownership when staging SQL

Status: accepted for the experimental synchronous query library.

Verified schema migrations need their SQL and receipt in the same original-engine
transaction. The current `execute` owns commit/rollback, so a later receipt could
otherwise be written in a second commit and diverge after a crash. Exposing a
borrowed `&mut Transaction` runner is insufficient: binding/planning errors are
query errors and do not necessarily mark the engine transaction aborted. Catching
such an error could leave earlier caller/SQL writes committable.

Add `stage(Transaction, sql, parameters) -> StagedScript` using an exclusively owned
transaction. Validate/parse/resolve/run only after ownership transfer. Every error
drops that owner, so prior writes cannot be recovered by the caller. On success,
provide borrowed bounded results and a consuming `into_parts`; callers may add
normal typed writes and commit once. Subsequent typed write failures use the
existing abort-on-write-error engine rule. Dropping success never commits.

Reject all SQL transaction-control statements in this API, including wrappers.
The caller is the sole commit authority. Share the same private statement runner
with existing `execute`, preserving its wrapper/report behavior. Keep per-call
query budgets and aggregate original transaction event/page/WAL limits explicit;
this does not add a global memory/work reservation or a network-held transaction.
No parser, database event, WAL or backup format changes.

Acceptance includes typed prefix/SQL/suffix visibility, exact unchanged history
on every error/drop/rollback, shared event capacity, work/output refusal, old
snapshots, checkpoint/verified restore, an independent generated committed model
and real killed children before/after commit on both WAL versions. Extend the
existing SQL mutation fuzzer to include typed prefixes and dropped successful or
failed staged scripts. Receipt integrity/version ordering and the migration CLI
remain the next separate implementation step. No production gate is closed.
