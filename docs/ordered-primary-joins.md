# Ordered, bounded primary-key joins

Eligible inner joins already probe the unique right primary key. They now preserve
a requested unique left primary prefix without collecting and sorting every match.
See [ADR 0058](adr/0058-streamed-primary-join-order.md).

```sql
SELECT a.id, b.label
FROM entries AS a JOIN labels AS b ON a.label_id = b.id
WHERE a.id >= $1 AND a.id < $2 AND b.active = TRUE
ORDER BY a.id DESC, b.label ASC LIMIT 20;
```

The left unique key gives an unambiguous order: at most one right primary match
can exist per left row. The cursor reads the requested end of the necessary left
range, evaluates complete ON then WHERE, and stops after 20 accepted matches.
Missing/null keys and rejected predicates still consume the script work budget.
A necessary exact left key uses one point lookup. OR/NOT are not inferred as bounds.
Nonzero primary-column positions and aliases use the resolved schema layout.

No-sort and proven left-primary-order plans retain only projected result fields.
A narrow selection can return IDs from wide rows without retaining hidden payload
for every match. The 10000-row and shared 8-MiB output caps still apply. Each
candidate remains temporarily assembled for full predicate evaluation; this is
not a whole-process or transient-memory reservation. Sorting by a right column
or another left column still uses the common stable sort and full intermediate
8-MiB cap before LIMIT. General fallback joins keep their existing behavior.

Integer extremes, short/long UTF-8/NUL keys, empty/contradictory ranges, complete
LIMIT 0/type/parameter validation, older immutable views and WAL 1/2 reads are
checked. Long keys keep validated physical-location access. EXPLAIN remains
access=primary_join with the same fields; requested ordering still sets sorted.
No grammar, SDK, API route, stored bytes or durability acknowledgement changes.

Warmed native allocation samples, generated map parity and bounded ASan campaigns
are recorded in [testing](testing.md). Cold cache/staging/model/transient budgets
and the combined durable writer remain open; this is experimental software.

On the 1500-by-1500 warmed release fixture, LIMIT 2 has requested peaks 8411
bytes for ordered source, 8769 for its restricted range and 8222 for one point.
Results retain respectively 348, 348 and 284 bytes and release to zero.
[Exact source-bound observations](measurements/2026-10-07-ordered-joins/operation-peaks.json)
exclude fixture/cache construction, allocator overhead, stacks and profiler data.
