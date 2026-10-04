# ADR 0030: borrowed mutation selection within remaining event capacity

Status: accepted for existing bounded SQL UPDATE/DELETE execution.

## Context

The mutation matcher cloned the complete candidate table/range, then accumulated
every matching row before applying writes. Normal transactions already permit
at most 256 events, so most of those retained rows could never be committed.
A real isolated Linux regression reproduced an allocation abort while selecting
6000 synthetic rows with 3072-byte payloads under eight MiB of extra headroom.
The staged snapshot and derived cache had already been constructed before the cap.

## Decision

Expose checked remaining normal event capacity on a ready Transaction. This is a
read-only observation, not a reservation, and does not replace changed-page/WAL
byte limits. Successful create/drop/insert/update/delete each consume one event;
reads do not. Aborted/poisoned owners remain errors, and a new transaction starts
with full normal capacity.

Bind schema, assignments, the entire filter and parameters before selection.
Use the existing equality-priority/necessary-range rules with checked borrowed
points or primary rows. Read ascending live primary order, including long keys,
and evaluate the complete three-valued predicate for every consumed candidate.
Reject the first TRUE candidate beyond remaining capacity before materializing
it. UPDATE accumulates only selected keys/row copies; DELETE accumulates keys
only. Keep that bounded immutable selection separate from applying mutations.

Earlier statements' staged events count toward the same limit. At zero capacity,
an UPDATE/DELETE with no matches remains a valid zero-event statement, subject to
normal binding/work checks. Full SELECT semantics and predicate work accounting
are unchanged. The private matcher returns a typed transaction Limit; execute
drops its owned transaction on failure, preserving complete script atomicity.

## Consequences and verification

Selection no longer clones a complete source vector or buffers more selected
events than can be attempted. Capacity checks may reject a broad mutation earlier
than the old matcher, which buffered it and failed during application. No partial
ACK or implicit batching is added. Staging still copies the original bounded
snapshot; this is not an eight-MiB limit for the whole process or SQL invocation.
Per-row physical validation and cold tree construction retain their documented
costs. Values/schema/row-size limits bound the selected key/row copies.

The original memory-capped child exits with SIGABRT on a 3072-byte allocation;
the repaired child returns Limit after 257 candidate visits, checks the key-only
path and a narrow range, and keeps the staged snapshot readable. Only the child
changes address/data limits and disables core dumps. Rustix process support is
test-only. Public capacity tests cover every mutation, reads, rollback, full
capacity and aborted views. SQL checks cover DDL/prior-write accounting, exact
256/257 boundaries, zero matches, full binding and unchanged failed WAL/history.

A 32-case independent model exercises integer/text/long/NUL/Unicode keys,
AND/OR/NOT, nullable predicates, updates/deletes, prior writes, rollback/error,
reopen/cache and both-version verified restore. Actual CLI and HTTP cases verify
long points, remaining capacity, generic errors, scope/log redaction and accepted
kill replay. A filesystem-backed ASan target generates bounded mutation scripts,
compares independent states and validates arbitrary SQL error atomicity/reopen.

Persisted formats, HTTP/SDK shapes and the 256-event limit remain unchanged.
Durable table-index WAL, wider fault/media/load/security campaigns and production
acceptance remain open.
