# ADR 0058: stream eligible joins in unique left primary order

Status: accepted for the original SQL subset. Storage, WAL and SQL grammar unchanged.

## Reproduced problem

After primary probes, a valid 1500-row-per-side fixture with 3072 hidden bytes
per row still fails ORDER BY a.id DESC LIMIT 2 with the 8-MiB intermediate cap.
This regression was run against the published preceding implementation before
changing the plan. Merely stopping its source after two matches fixes that case,
but a second regression returning all 1500 IDs still fails when full hidden joined
rows are retained. Both fail before their respective corrections and pass after.

## Decision

The necessary equality to a unique right primary key emits at most one joined row
per left row. Therefore a first ORDER BY column equal to the resolved left unique
primary column completely determines output order. Secondary sort/null rules have
no ties to resolve. Borrow the original double-ended primary cursor in the selected
direction and stop after LIMIT accepted matches. Null/missing probes and failed
ON/WHERE candidates do not count towards LIMIT and still charge work.

The same eligible plan may use existing necessary WHERE bounds on the left primary
column: exact point lookup, or an inclusive/exclusive integer/text range. Extract
only proven necessary comparisons under AND. Retain full ON then WHERE evaluation;
OR/NOT do not create inferred source bounds. Resolve all types, columns, aliases,
parameters and LIMIT before opening the cursor, including empty input and LIMIT 0.
Long keys preserve validated map/physical-location access and original UTF-8 order.

When no sort is requested or this unique left prefix proves order, retain only
selected output columns and charge their bytes once to the shared script budget.
One full joined candidate remains temporary for predicate evaluation. Retained
output remains capped at 10000 rows and 8 MiB; this does not reserve whole-process
or transient memory. Other ORDER BY prefixes retain the full common stable sorter,
its intermediate 8-MiB cap and result budget. Other joins retain the original
bounded nested loop. No heuristic right-key uniqueness or early LIMIT is inferred.

EXPLAIN retains access=primary_join and its previous fields; sorted still describes
requested ordering. No new client enum, endpoint, dependency, unsafe code, allocator
or persistent format is introduced. This extends [ADR 0057](0057-primary-key-join-probes.md).

## Evidence and limits

Private actual-work checks verify two ordered matches need six visits, one left
point four, and a two-match range twelve. Empty contradictory ranges visit zero.
Right/non-primary ordering still visits every eligible source; false filters and
shared output exhaustion still refuse. Public independent 64-case map/fallback
comparisons cover nullable many-to-one data, both directions, nonzero primary
positions, null/tie rules, bounds, typed empty input, long/NUL/UTF-8 keys, integer
extremes, old physical images and full global row capacity. Repeated managed read
scripts use both WAL versions and preserve exact committed journal bytes through
checkpoint/reopen. Source-bound warmed diagnostics and ASan results are recorded
in [testing](../testing.md), separately from cold cache construction.

No broader JOIN grammar, hash/merge join, secondary-index DDL, combined durable
index writer, model/cache/staging/transient quota or production acceptance is implied.

## Subsequent sorting extension

[ADR 0059](0059-bounded-primary-join-sort.md) subsequently replaces the full sorter
for other eligible primary_join order prefixes with bounded stable selection.
The original retained byte/matched-row/work/output allowances remain active;
all necessary source candidates still execute. Earlier measurements stay historical.
