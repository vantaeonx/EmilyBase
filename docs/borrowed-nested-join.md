# Nested joins without copied source tables

The original nested-loop fallback now borrows checked source rows. Bounded
vectors retain references, and immutable RowView supplies complete ON/WHERE
values. Rejected pairs create no combined owned row. True matches still pass the
original full-row byte/count admission before cloning, stable full sort, LIMIT
and shared projection budget. Every pair and Boolean expression branch keeps
its original work charge. See [ADR0066](adr/0066-borrowed-nested-loop-sources.md).

This does not change the planner or relax fallback memory/work limits. Left
filters remain full-scan filters in public fallback plans. Small sorted LIMIT
can still refuse if full matched intermediates exceed the original bounds.
Primary-key probes retain their asymptotic advantage over nested pairs. Long
keys, aliases, self joins, NULL truth, stable ties and read-only old views remain.

A real native regression first fails on da65a95: with100 rows per source and
3000-byte hidden payload, false ON/WHERE request64,469,242/64,468,894 bytes.
The repaired samples request9530/9182, peaks3960/3608/drop0. The100-match sorted
sample falls64,475,392→634,880, peak944,680→626,040. Cold fixtures/indexes/fingerprints
precede tracking; the [source-bound observation](measurements/2026-10-07-borrowed-nested-join/operation-allocations.json)
records exclusions. These are summed operation allocations, not RSS, throughput,
cold heap, total stack use or server quota.

The existing4000-row comparison now permits borrowed fallback peak80,088 rather
than requiring more than16 MiB of copying; primary peak2622 remains separately
measured. Historical artifacts keep their original source hashes and figures.
No mandatory WAL, durable ACK, file/cache version, SQL or HTTP change occurs.
Numeric model/cache/staging/transient budgets and durable-writer work remain.
