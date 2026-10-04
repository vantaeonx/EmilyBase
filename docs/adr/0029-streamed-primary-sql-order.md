# ADR 0029: streamed primary SQL ordering and filtered limits

Status: accepted for bounded single-table SELECT execution.

## Context

The executor materialized every candidate and sorted complete rows before
applying ORDER BY/LIMIT. An actual regression on 6000 rows with 3072-byte hidden
payloads rejected `SELECT id FROM t ORDER BY id DESC LIMIT 2` at the 8-MiB
intermediate estimate. Sixty-four such small reads also spent work on every row.
Borrowed live-row cursors now provide checked ascending/descending primary order.

## Decision

After complete schema, predicate, parameter, projection, order and limit binding,
select the borrowed path for single-table reads without explicit ordering, or
when the first ORDER BY field is the primary column. The primary key is unique
and non-null, so later terms cannot break ties; still bind and validate all of
them. Equality lookup retains priority. Otherwise use extracted necessary range
bounds, or the full primary interval, reading from the appropriate end.

Validate each consumed current physical image. Long live keys retain their
ordered-map positions. Long point equality checks its row location/image too.
Evaluate the complete three-valued predicate before counting a match. Stop only
when the requested number of TRUE matches has been returned, or the source ends.
Clone projected fields only; charge each retained projected row to the existing
shared script output budget. Charge row/predicate work for every consumed row,
including discarded matches. LIMIT zero returns no rows after complete binding.

Joins and orderings whose first field is not the primary key retain the bounded
materialized executor and stable tie/null semantics. Their intermediate limits
still apply before final LIMIT. Mutation selection retains its prior range path.

Reuse the existing `primary_range` explain access for primary-ordered intervals,
including an unbounded interval. `sorted` continues to indicate requested ORDER
BY, rather than allocation of a sorting buffer. Point and join descriptions keep
priority. No new HTTP/SDK fields, syntax or persisted bytes are introduced.

## Consequences and verification

Ordered small results no longer require a complete copied candidate vector or
whole-row sort. Output remains bounded by estimated retained row bytes across
the script; this is not wire-size accounting. Derived tree construction, the
already retained snapshot and bounded per-row event decoding are outside that
output/work counter. No throughput or zero-allocation claim follows. A partial
read checks consumed physical images; complete cache import validation remains.

The failing wide-table case is reproduced before repair. Independent integer
and text models exercise necessary AND, OR/NOT, NULL, aliases, reversed bounds,
non-leading primary columns, all directions/limits and long/NUL/Unicode keys.
Typed star projections retain bytes, booleans, nulls, extreme integers and exact
negative-zero float bits. Missing fields/bindings/type errors are checked on
empty inputs, contradictions and LIMIT zero. Non-primary ordering and joins
remain on their previous executor.

Actual shared work/output limits, staged reads, rollback/error atomicity, old
snapshots, caches/reopen and verified backup/restore run under WAL 1/2. Compiled
CLI and actual HTTP wide reads verify plans, read-only WAL preservation, denied
project/master scopes, log redaction and ACK replay after kill. The existing
SQL ASan target adds independent mixed predicates, projection/order and long-key
models. Smoke fuzzing does not close broader recovery/load/security gates.

Durable table-index WAL, secondary DDL, MVCC, arbitrary ordering optimization
and production acceptance remain separate work.
