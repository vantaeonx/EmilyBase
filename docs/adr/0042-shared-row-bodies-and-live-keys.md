# ADR 0042: share immutable row bodies and live keys

Status: accepted. Structural map copies and heap/lifetime admission remain.

## Context

ADR 0041 shares unchanged tables, but first mutation still deep-copies all keys
and row bodies in its affected table/location map. Four held long-key models
still peak near 940 MiB when each replaces one row. A row-body pointer regression
reproduces the unnecessary copying of unrelated rows within the same table.

## Decision

The private table map now stores immutable Arc<Key> and Arc<Row>. Its per-table
location map also shares immutable keys. Detaching a map clones structure and
Arc handles; insertion/replacement creates only the new owned row body. Existing
keys/rows remain shared through old snapshots. Neither mutable references nor
internal Arc types escape the public database API.

Borrowed get/cursors still return checked &Row; scan/range APIs still return
independent owned Row copies. Key comparisons and explicitly borrowed Key ranges
retain integer/UTF-8 ordering. Derived B+ building clones only eligible keys into
its original arena; independent topology and physical-image validation remain.
Raw-file CRUD, managed WAL, backup, query and server paths use the same row values.
Stored bytes, checksums, fingerprints, transaction fences and API shapes are
unchanged. No unsafe block, database dependency or new file format is introduced.

## Consequences and verification

Shared body/key identity is checked after first mutation, deletion/reinsertion,
rollback and many generations. Full 10000-row cases exercise integer, 256-byte
and 3072-byte key boundaries, exact current pointers and both cursor directions.
Owned scan mutation cannot affect a snapshot/raw file. Weak references confirm
objects release after their last live generation. An independent 32-case row
model preserves unrelated row identity through accepted/refused/discarded changes.
Complete recovery/backup/isolation suites remain required before publication.

Map-node copying is still O(rows in the affected table), derived B+ copies remain,
and Arc allocations/refcount work increase per-row baseline costs. Many old
generations retain map structures and changed bodies; sharing does not enforce
a quota. Local optimized synthetic measurement reduces the four-model requested
peak from 985288974 to 581147606 bytes, with baseline growing from 572912112 to
574193520. This is shape-specific evidence, not concurrent-worker/replay admission
or a throughput comparison. See [profiles](../model-allocation-profiles.md).
