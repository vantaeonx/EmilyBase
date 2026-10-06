# ADR 0052: validate primary export without complete image reconstruction

Status: accepted for derived primary projection. Runtime formats are unchanged.

## Context

The relational snapshot exports its derived primary tree by encoding every page
into a complete image vector, then decoding another owned arena. ADR0051 already
provides immutable page ownership. The warmed10000-key text export still fails a
128-KiB operation guard at7480176 requested bytes because of the intermediate set.

## Decision

Provide BPlusTree::to_stable as a fully checked view conversion. Refuse an empty
or oversized arena before map copying, copy only the bounded private handle map,
set stable allocation/deletion policy only on that returned map, then perform
the existing complete index admission. Exact bounded map/page IDs, root, entry
count, keys/pointers/occupancy/topology/leaf links and per-page wire round trips
remain mandatory. The method preserves IDs, holes and bytes; it does not repair
malformed count/identity or normalize sparse IDs.

Relational export uses that conversion instead of a complete physical image set.
It still verifies every eligible live row/key/current pointer against the exact
relational state and counts long-key exclusions. The original derived cache stays
immutable and retains its own stable/dense policy. External writes detach exported
pages; source writes detach cache pages. Neither can stale historical rows.
Explicit installation still performs its original complete coverage verification.

## Verification and limits

The original export regression runs and fails before the implementation change.
Its warm10000-row/256-byte-text/768-page operation now observes51680 requested peak
bytes, including returned map ownership and a second explicit coverage check.
The complete retained relational fixture and first cache build are excluded.
Operation-local requested current bytes return to zero. Optional instrumentation
remains outside the ordinary server/CLI.

Private tests verify real shared page owners, original import equivalence, exact
last arena ID, sparse holes, source release, dense/stable policy isolation and
malformed identity/count/topology/physical refusal. Independent generated histories
match rows and original decoded imports. Relational tests cover Unicode/NUL/long
keys, stale pointer refusal, actual four thread snapshots and retained historical
projections. Automatic model rebuild tests verify unchanged-root and changed-leaf
prepare/replay through the actual public rebuild path.

The map and transient checks still allocate. Cold cache construction and decoded
rows/history are outside this operation guard. Source/external retention and OS
stacks still need separate admission. This optimization makes no worst-case heap,
throughput, stable-release or production claim, and selects no new runtime WAL.
See [observations](../shared-primary-export.md).
