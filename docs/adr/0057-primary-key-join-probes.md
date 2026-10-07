# ADR 0057: probe the unique right primary key for eligible inner joins

Status: accepted for the documented original SQL subset. Storage/WAL unchanged.

## Reproduced problem

The bounded nested-loop executor cloned complete source tables and visited each
left/right pair. A valid 400-row-per-side equality join on the right primary key
failed with Limit("query work") under the existing 100000-visit script budget.
The valid regression was executed against the preceding published implementation
before enabling the new plan, and passes with primary probes. The initial fixture
used an incorrect event variant; it was corrected before that baseline execution.

## Decision

After complete schema, projection, predicate, ordering and parameter resolution,
extract a necessary equality between any left column and the right primary column.
Equality may be reversed and appear under AND. Do not infer necessary equality
from OR/NOT, same-side comparisons, literals or non-primary columns. Type validation
still precedes plan selection, including empty tables and LIMIT 0. Aliases and a
nonzero primary-column position use the already resolved joined layout.

Stream left rows in their original primary order. Charge work for every probe,
including null/missing keys. Null equality cannot match. Use the original point
path for the unique right row: the B+ tree covers eligible integer/short-text keys;
long text retains the ordered map and physical row-location check through 3072
bytes. No complete source scan/copy is constructed for this access path.

For a found row, charge candidate work, assemble one joined row and evaluate the
complete ON and WHERE predicates. Preserve existing joined projection metadata,
stable sort/null/tie rules, unordered LIMIT and intermediate/result row/byte limits.
Use the common final sorter/projector. Do not move WHERE ahead of ON or weaken the
shared script budget. Other joins retain the bounded nested-loop executor.

EXPLAIN reports access="primary_join" for eligible plans. The project TypeScript
client accepts this additional enum value with its existing strict decoding; real
HTTP/client checks exercise both EXPLAIN and SELECT. Strict enum consumers need
the matching client update. No fields, routes, credentials or file versions change.
The only new diagnostic dependency is query as a model-profile dev dependency;
no database engine, async storage or runtime allocator is added.

## Evidence and scope

Actual executor counters for a complete 200-by-200 equality join are 600 versus
80000, with identical rows/output charges. Private checks also charge null/missing
probes and every ON/WHERE node, and refuse exhaustion of the original shared budget.
Public checks preserve reversed equalities, nullable/repeated/missing keys,
nonzero primary positions, self aliases, star labels, filters, ordering/limits,
Unicode/NUL/255/256/257/3072-byte keys and retained old views. A 64-case independent
many-to-one map verifies complete output and compares the original fallback.

The sorted fast path still refuses full intermediate data above 8 MiB even with
LIMIT 1. Both managed WAL versions execute staged reads, rollback with exact WAL
preservation, commit, old readers, checkpoint/compaction, reopen and verified
backup/restore with independent subsequent writes.

A release-native sample uses 4000 rows per side with 3072 hidden text bytes per
row and LIMIT 2. The equivalent OR FALSE predicate deliberately selects the
original fallback. Requested peaks are 25550648 versus 14328 bytes; both retain
348 output bytes and return to zero after result release. Complete fixture/cache
construction precedes profiling. This compares warmed operation-local allocations,
not cold index construction, allocator usable-size, RSS or a process quota.

Fallback work limits can still refuse valid expensive joins; no hash/merge join,
left-primary reorder, secondary index, outer/chained join or PostgreSQL protocol
is introduced. Cold cache/staging/durable-index/production gates remain open.
