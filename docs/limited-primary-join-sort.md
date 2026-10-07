# Bounded sorting for primary-key JOIN results

Eligible primary joins can now sort a small LIMIT by right-side or non-primary
columns without retaining every full matching row. See [ADR 0059](adr/0059-bounded-primary-join-sort.md).

```sql
SELECT a.id, b.rank
FROM entries AS a JOIN labels AS b ON a.label_id = b.id
WHERE a.id >= $1 AND b.active = TRUE
ORDER BY b.rank DESC NULLS LAST, a.id ASC LIMIT 20;
```

The executor evaluates complete ON/WHERE for the necessary source, keeps only
its best 20 full candidates, then returns selected fields in requested order.
It still examines every admitted source candidate; LIMIT is not an early exit for
this ordering. Equal requested keys preserve the original source order. Boolean,
integer, finite float, text, bytes, NULL position and secondary keys share the
original full-sort comparator. Unique left-primary ordering keeps its separate
[streamed cursor](ordered-primary-joins.md).

The max heap is an in-memory selection algorithm from the standard library; the
original storage engine and its B+ tree remain authoritative. Full retained row
estimates stay within 8 MiB and every accepted match consumes the original 10000
intermediate-row allowance. Work remains script-wide; projected output uses its
own shared 8-MiB budget. A larger LIMIT can still be refused. A discarded candidate
is temporary and not charged as retained; heap capacity/metadata, cold caches,
models and allocator overhead are outside that row-byte estimate.

The valid wide-row LIMIT 2 regression fails before the change and passes after.
Generated independent models, exact fallback parity, all sort types, stable ties,
old images, necessary source bounds and WAL 1/2 atomic staged reads are checked.
Warmed native requested peaks are 21051, 21053 and 21055 bytes for right/left
non-primary/tied sorts, retaining 444 bytes and releasing to zero. Exact source
hashes and exclusions appear in [observations](measurements/2026-10-07-limited-join-sort/operation-peaks.json).
See [testing](testing.md) for actual suite and bounded ASan results.

Single-table sorts and non-primary fallback joins keep their previous full-sort
limits. No grammar, client enum, API, stored format or durable ACK changes. This
is experimental software; combined durable writer and whole-memory gates remain open.

[Projected-output admission](projected-output-admission.md) subsequently checks
selected bytes before cloning and projects final rows incrementally. Measurements
above bind the earlier source; current admitted-refusal observations are separate.
