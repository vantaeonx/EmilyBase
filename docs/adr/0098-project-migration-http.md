# ADR0098: Project-service migration HTTP over retained original data owners

Status: accepted for experimental trusted service transport.

Expose GET migrations and POST migrations/apply under both project routers.
Reuse the synchronous original migration crate, strict complete ledger inspection,
exact definition hash and atomic SQL/receipt commit. No new storage/WAL codec,
background worker, database engine or private metadata table is required.

Generalize the existing retained project table operation to a typed data operation;
table calls delegate without changing their error contracts. Keep exclusive data
ownership and the per-project gate across open/recovery, parsing, application and
serialization in an admitted blocking worker. Private-root mode checks readiness
and current service credentials after body waits and before decoding. Legacy
admitted capabilities preserve their existing rotation policy. Neither mode
observes a private session clock or writes private history for migration traffic.

Use strict bounded JSON version/label/sql. Return only bounded receipt metadata:
version, label, lowercase64-hex digest and canonical decimal-string u64 transaction.
Inventory returns all at most128 receipts. Existing65536-byte body/output limits,
five-second body deadline, four workers, rates, static errors/logs and no-cache
headers apply. Master/access/refresh credentials are not project service authority.

Ordinary admission/order/conflict/execution errors map to400 migration_rejected.
Invalid ledger maps to503 migration_history_invalid. Preserve ambiguous original
write/response classification as503 transaction_outcome_requires_inspection; never
turn it into a safe400 refusal. No repair, automatic retry or inferred rollback.
Concurrent identical definitions share one serialized commit and original receipt.

Acceptance includes strict pure decoder fuzzing, full receipt bounds/u64 metadata,
static error matrix with actual ambiguous-WAL error types, sibling/current-key
isolation, delayed malformed-body rotation, simultaneous exact retries, real TCP
received-ACK kills on both routers/WALs, and common-root compaction/backup/restore
with complete receipt comparison. Local native checks and hosted containers are
separate verification layers. Online coordination, schema diff/down, RLS, resource
admission, wider power-loss/security/upgrade and production gates remain open.
