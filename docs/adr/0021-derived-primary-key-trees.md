# ADR 0021: derived primary-key B+ trees in managed snapshots

Status: accepted for point lookup; durable index pages remain pending.

## Context

Validated row locations now reject obsolete history images. The original bounded
B+ tree can route integer and up-to-256-byte text keys to actual page/slot records.
Its incremental 1024-page arena can fill before 10000 entries; table text keys
still permit 3072 bytes. Integrating routing must preserve admitted data and
immutable snapshot/rollback behavior without changing any published file format.

## Decision

Each managed relational snapshot holds a per-table Arc/OnceLock derived tree.
The first eligible point lookup or explicit inspection builds it bottom-up from
sorted live keys and their validated locations. A full 10000-key build uses 768
pages, within the current arena bound. Integer and text keys up to 256 UTF-8 bytes
use original B+ tree routing; longer valid text keys use the retained row map.
Mixed tables retain both paths and report excluded long-key counts explicitly.

Found pointers must equal the current live page/slot and resolve through the
event fingerprint/table/key/row checks. A missing indexed live key or mismatched
pointer fails with a typed invariant error, never a stale row. Row maps remain
the validated relational representation and supply scans/long-key lookups.
SQL SELECT and filtered UPDATE/DELETE already call Snapshot::get for primary-key
equalities, so these existing paths use the adapter without changing wire plans.

Immutable snapshots share initialized trees. Successful mutations prepare index
maintenance on a private tree copy before publishing relational state/pages/maps.
Pointer replacement, insertion and deletion use the original tree operations;
only the changed table cell is replaced. Failed writes publish none. Long-key
changes may retain an initialized eligible tree. Uninitialized cells are replaced
before changes so a historical branch cannot later initialize a shared stale tree.
Create/drop reset/remove their table cell; rollback drops the entire staged view.

If incremental maintenance reaches the arena limit, discard only its derived
cache; the next point lookup builds densely from the accepted live rows. Other
maintenance inconsistencies fail before mutation. No arena limit narrows existing
table admission. PrimaryIndexInfo/CLI expose counts/root/pages; these runtime
arena statistics can differ after reopen, which builds a canonical dense tree.

## Consequences and remaining work

Pages/events/WAL/backup bytes remain unchanged. Recovery, both WAL versions,
baseline compaction and verified restore rebuild the derived cache from committed
history; it is not a separate durable truth. No sidecar file, secondary index,
CREATE INDEX, persistent catalog root or atomic index-page WAL write is introduced.
Range/order scans continue to use validated row maps. Those persistent features
need a separate format/allocation design and recovery matrix.

Lazy construction costs O(n) once per initialized table snapshot. Maintenance
currently copies a bounded arena and deletion validates it; this is a correctness
baseline, not a throughput claim. Cache memory is bounded by existing table/row,
key and arena limits. Query work/output limits describe execution, not initial
cache construction or all allocator overhead. Network filesystem/SQL work still
runs on blocking workers. Pure snapshot readers may share OnceLock; this adds
neither MVCC nor concurrent filesystem owners.
