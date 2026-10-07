# ADR 0062: evaluate and admit borrowed candidates before cloning payload

Status: accepted for the synchronous original executor. Durable formats unchanged.

## Reproduced problem

Bounded ordinary sorting still cloned every matched candidate before the heap
could discard a worse row. Primary joins concatenated cloned left/right payloads
before complete ON/WHERE, even for rejected candidates. Against published 281c01d,
a warmed release fixture with 1500 rows per table, 3072 hidden bytes and LIMIT 2
observes requested allocation totals 9819837/19937652/19937358 for ordinary sort,
sorted primary join and filtered primary join. Its native total-allocation guards
actually fail before implementation. Peak memory alone hides repeated allocation.

## Decision

Introduce a private immutable logical row view over one or two validated source
slices. Checked logical offsets resolve left fields first and right fields after
left width; usize::MAX or malformed internal offsets return typed plan errors.
The view carries references only, never a copied cell vector, and cannot outlive
its source. It changes no public ownership API and contains no unsafe code.

Predicates evaluate that view through the original recursive implementation.
Every Boolean branch still executes, including false AND/true OR, preserving
three-valued SQL logic, type failures and exact shared work charges. Primary
probe/physical validation remains mandatory before the view is used. Streamed
primary joins project selected values directly through the existing pre-copy
output admission. No full joined row is necessary for rejected/streamed matches.

Bounded table/primary-join heaps compare borrowed values with the worst retained
row using the same null/type/direction comparator as owned stable sorting. Equal
keys lose to earlier source ordinals. Every accepted match still consumes the
original intermediate-count limit, including discarded candidates. Only a winner
is charged against the unchanged retained-row-byte formula; growth/replacement
refuses before copying payload or changing the previous selected set. After
admission, the heap owns its winning row, so cursor movement cannot invalidate it.
Projection reserves exactly the selected cell count after output admission.

Binding, explicit field caps, all supported types/float bits, SQL/API contracts,
EXPLAIN, ordering, output/matched/work limits and error strings retain their
meanings. General fallback joins keep their materialized implementation. No WAL,
cache/file bytes, commit acknowledgements, async storage or dependency changes.

## Evidence and remaining costs

The same warmed samples now request totals 5074173/10302324/10289358; peaks are
11634/17980/5180, live output 282/282/122 and after-release zero. Exact fixture/hash
bindings are in [observations](../measurements/2026-10-07-borrowed-candidates/operation-allocations.json).
Physical source validation still allocates; replacement winners, long probe keys,
selected output and metadata retain their existing costs. These are requested
operation-local allocations, not usable allocator size, throughput or RSS.

Private checks cover all split offsets/types, shared comparisons, malformed
internal indices, original work/three-valued predicates, exact repeated projection
admission and heap refusal atomicity. Public checks join two 64-column schemas,
including primary fields at final position, all 128 star fields, aliases/repeated
cells, old views and complete nullable filters. Both WAL versions preserve staged
changes to both sources, rollback, cumulative output refusal, exact committed
bytes, checkpoint/reopen and independently writable verified backup/restore.
An independent nullable model and original fallback cross-check seed bounded ASan.

Numeric model/cache/staging/transient reservations and the combined durable writer
remain open. This closes no production, hardware power-loss or whole-memory gate.
