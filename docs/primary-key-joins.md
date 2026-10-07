# Primary-key inner join access

EmilyBase can probe its original primary index when a resolved inner JOIN has a
necessary equality to the right table's unique primary column. See
[ADR 0057](adr/0057-primary-key-join-probes.md).

```sql
SELECT a.id, b.label
FROM entries AS a JOIN labels AS b ON a.label_id = b.id
WHERE b.active = TRUE
ORDER BY a.id DESC LIMIT 20;
```

The equality can be reversed or required by an AND expression. The complete ON
and WHERE conditions still execute. Qualified columns, aliases/self joins and
primary columns in any schema position are resolved before access selection.
OR/NOT, same-side equality, literal-only and non-primary comparisons keep the
bounded nested loop. No chained/outer joins or secondary-index DDL are added.

Left rows keep their original ordered borrowed scan; each non-null foreign value
probes at most one right row. Short text/integer keys use the own B+ tree. Text
keys excluded from that tree keep the validated map/physical-location path through
3072 bytes, with identical UTF-8/NUL ordering and no Unicode normalization.
Missing and null probes still consume work. Every candidate and full ON/WHERE node
uses the shared 100000-visit script budget. Unknown/ambiguous columns, type errors
and missing bindings are refused before scanning, including LIMIT 0/empty tables.

Projection, stable sort, null/tie handling and limits keep their previous meaning.
The full intermediate-byte cap still applies when sorting before LIMIT; returned
projection bytes also use the script-wide budget. Immutable older snapshots and
managed staged/rollback/recovery/backup reads use the same plan.

EXPLAIN returns access="primary_join", with the previous field set. The matching
TypeScript SDK accepts the additional value; unknown values remain refused. Strict
older enum consumers need a client update. No endpoint or persistent format changes.

## Checked work and allocations

For a complete 200-row-per-side equality fixture, actual executor counters fall
from 80000 to 600 while complete rows/output charges remain equal. A regression
that exhausted the previous nested-loop budget at 400 rows per side now passes.
Those counters count logical probe/candidate/predicate visits, not all CPU work
inside cache construction, row validation or B+ traversal.

A warmed release sample with 4000 rows per side and 3072 hidden bytes per row,
LIMIT 2, observes requested allocation peaks 25550648 for the equivalent fallback
and 14328 for probes. Both retain 348 output bytes and return to zero after release.
The fixture and both caches precede profiling. [Source-bound observations](measurements/2026-10-07-primary-joins/operation-peaks.json)
exclude their cold construction, allocator overhead/rounding, stacks and profiler
bookkeeping. This is neither a cold-memory estimate nor a model/server heap quota.

Generated map comparisons, fallback parity, long keys, stale old views, bounds,
managed WAL 1/2 and actual HTTP/SDK validation are documented in [testing](testing.md).
General fallback joins still retain their existing limits and can be refused.
