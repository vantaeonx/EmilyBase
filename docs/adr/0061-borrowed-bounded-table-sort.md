# ADR 0061: borrow ordinary sorted sources and retain only LIMIT candidates

Status: accepted for the original synchronous SQL executor. Durable rules unchanged.

## Reproduced problem

An ordinary non-primary ORDER BY cloned its complete source before sorting. A
valid 2800-row table with 3072 hidden bytes per row and LIMIT 2 failed the original
8-MiB intermediate-byte limit against published 72c97fa. A separate warmed
1500-row release sample succeeded but requested a peak of 4842026 bytes; its
128-KiB operation guard failed before implementation. Neither regression changes
schema or asks the engine to raise a resource limit.

## Decision

For a fully bound single-table SELECT with a non-primary ORDER BY, borrow checked
rows from the original primary cursor, apply complete WHERE and retain only the
best LIMIT full candidates in the private stable selection heap from ADR 0059.
Necessary primary points/ranges restrict the source. A point still uses checked
physical lookup, including long keys; a contradictory interval visits no rows.
All admitted candidates must be examined because this order cannot stop after
the first LIMIT matches. Aliases and nonzero primary-column positions remain bound
by the planner. EXPLAIN keeps its existing scan/primary_key/primary_range contract;
no access enum, grammar or client type is introduced.

The heap uses the original sort comparator for Boolean/integer/finite float/text/
bytes, explicit NULL placement and secondary keys. A source ordinal preserves
stable ties. Ascending primary source order matches the previous materialization,
including mixed short/long UTF-8 and embedded NUL keys. Every matching candidate
consumes the original 10000 intermediate-row limit even when discarded. Work is
still shared across the script. Retained full-row estimates stay within 8 MiB;
larger LIMIT requests can still fail. Incremental projection checks the separate
shared 8-MiB output allowance before cloning selected values under ADR 0060.

Unique primary order/unordered streaming and eligible primary joins retain their
existing dispatch. General fallback joins retain their original materialized
limits. No cache, file format, WAL version, commit acknowledgement or concurrency
model changes. The synchronous storage layer remains separate from async HTTP.

## Evidence and boundaries

The valid 2800-row regression succeeds after repair. The same warmed 1500-row
sample now requests peak 11634 bytes; a necessary range requests 11990 and a point
5196. Each returns the expected rows and releases to zero. Fixtures and primary
caches are constructed before profiling. Exact source hashes and exclusions are
in [source-bound observations](../measurements/2026-10-07-limited-table-sort/operation-peaks.json).
These are requested operation-local allocations, not cold memory or RSS.

Actual work checks demonstrate 200 visits for a 200-row LIMIT 2 sort, 40 for ten
range rows with their complete predicate, 2 for a point and 0 for contradiction.
Public checks cover all sort types, stable ties, long primary keys, old snapshots,
unchanged resource refusals, full empty/LIMIT0 binding and independent nullable
models. WAL 1/2 scripts preserve staged reads, rollback, exact failed committed
bytes, checkpoint/reopen and verified independent backup/restore writes.

Temporary candidate clones, heap metadata, allocator rounding, cold caches,
model/staging allocations and whole-process admission are separate open gates.
Combined durable table/index publication and production acceptance remain open.
