# Borrowed sources for limited table sorting

```sql
SELECT id, rank FROM entries
WHERE id >= $1 AND id < $2
ORDER BY rank DESC NULLS LAST, id ASC LIMIT 20;
```

The original executor borrows physically checked source rows and retains the best
20 full candidates for this ordinary non-primary order. It evaluates complete
WHERE and every necessary source candidate; LIMIT does not permit early exit.
Primary points/ranges restrict access. Equal sort keys preserve original primary
source order. All supported value types, NULL placement, secondary keys and
aliases use the same comparator as the previous stable full sort.

Matching candidates still count toward 10000 intermediate rows. Work remains
script-wide. Retained full rows must fit the original 8-MiB estimate, and selected
output is independently admitted before copying under the shared 8-MiB output
allowance. Large LIMIT/output requests can still fail and roll back staged writes.
EXPLAIN keeps scan/primary_key/primary_range; sorted continues to describe ORDER BY.

A valid wide-row LIMIT 2 regression first fails against 72c97fa and then passes.
An isolated warmed release sample with 1500 rows and 3072 hidden bytes per row
requests peak 4842026 bytes before repair and 11634 after. Point/range samples
observe 5196/11990; all release to zero. Fixture/cache construction precedes the
measurement. These figures exclude allocator rounding/overhead, stacks and
profiler data and are not a cold-memory or process quota. Exact hashes and sample
dimensions are in [observations](measurements/2026-10-07-limited-table-sort/operation-peaks.json).

See [ADR 0061](adr/0061-borrowed-bounded-table-sort.md) and [testing](testing.md).
Primary-order streaming and bounded primary-join selection retain their existing
paths; general fallback joins retain their original limits. Models, staging,
cold caches, candidate transients, combined durable writer and production gates
remain open. No SQL grammar, API, stored format or durable ACK changes.
