# ADR 0059: retain only the best LIMIT candidates for primary-join sorting

Status: accepted for the original SQL subset. Persistent formats and ACK unchanged.

## Reproduced problem

An eligible 1500-row-per-side primary JOIN with 3072 hidden bytes per source row
and ORDER BY a right-side rank LIMIT 2 fails the full intermediate-byte limit.
The valid regression was executed against published ef6b62e before changing
selection. Unique left-primary streaming cannot resolve this requested order.

## Decision

For an eligible primary_join that needs a different ordering, use an original
stable bounded selector around the standard-library max heap. Keep at most LIMIT
full joined candidates, with the worst retained candidate at the root. A better
candidate replaces it; equal candidates prefer their earlier source ordinal.
The ordinal supplies the original stable-sort tie behavior without adding a SQL
sort column. Each heap entry borrows the same immutable, completely resolved order.
Final heap extraction supplies ascending requested order before common projection.

Share the exact null/direction/type comparator with the original full sorter.
Complete schema/parameter/type/projection/order/LIMIT binding still precedes reads.
Finite floats preserve bits, including signed zero. Full ON then WHERE evaluate
before selection. Every source probe, missing/null lookup, candidate and predicate
keeps its original work charge. No early termination is inferred for this ordering;
all admitted source candidates must be examined, even with LIMIT 1. Existing
necessary left-primary point/range access may restrict that source.

Retained full-row estimates remain capped at 8 MiB. Before inserting/replacing,
check the resulting retained-byte sum. A worse discarded candidate does not add
to retained bytes; replacing a larger retained row releases its old charge. A
refused insertion/replacement leaves the selected heap and retained-byte counter
unchanged. Every accepted matching candidate still consumes the previous 10000-row
intermediate allowance, even when discarded by the heap. Output projection bytes
remain separately charged once to the shared 8-MiB script output budget.

This reduces retained candidate bodies, not all transient allocations or process
heap. Joined rows are still temporarily assembled. Heap capacity/ordinals, maps,
cold caches, source state and allocator overhead are outside the row-byte estimate.
The limit never admits an unlimited sort or removes shared work/output caps.

Only non-streamed eligible primary joins use this selector. Unique left-primary
order/no-sort plans retain projected streaming from [ADR 0058](0058-streamed-primary-join-order.md).
Single-table sorts and general fallback joins keep their original stable sorter
and bounds. No new SQL grammar, EXPLAIN value, API, runtime dependency or unsafe
code is added. Existing mandatory WAL versions and stored bytes are unchanged.

## Evidence and limits

Seven private checks verify worst-root replacement, stable ties, both directions,
NULL position, original match-count exhaustion, retained growth refusal, larger
replacement refusal, discarded payloads and release after smaller replacement.
Public generated independent nullable many-to-one models and original fallback
parity cover both directions, NULL positions and complete filters/source bounds.
Typed public cases compare boolean/integer/finite-float/text/bytes sorting,
secondary keys, aliases, signed zero, old views and empty/LIMIT0 binding. Managed
WAL 1/2 staged reads, rollback, failed scripts, exact WAL preservation and
checkpoint/compaction/reopen execute with the same selector.

A warmed 1500-row-per-side sample observes requested peaks 21051/21053/21055 bytes
for right order, left non-primary order and equal-key ties, all LIMIT 2. Each
retains 444 output bytes and releases to zero. Complete fixture/cache construction
precedes profiling. [Source-bound observations](../measurements/2026-10-07-limited-join-sort/operation-peaks.json)
exclude cold construction, allocator overhead/rounding, stacks and profiler data.
These counters are not RSS, throughput or a transient/process quota.

A large LIMIT can still reach the retained-byte cap. General fallback joins can
still exhaust their original bounds. Wider model/cache/staging/transient admission,
combined durable table/index writer and production acceptance remain open.
