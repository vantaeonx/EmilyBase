# ADR0065: borrow complete schema inventories from immutable snapshots

Status: accepted for snapshot metadata and complete model validation.

## Reproduced problem

`Snapshot::schemas()` deliberately returns independent owned copies. Calling it
inside both `Staged::prepare` and `State::validated` copies all table names and
column vectors even when one index changes. A native regression against 881ca75
fails: a 128-table fixture with 64 columns per table requests 1,459,120 bytes,
17,177 blocks, and a 723,160-byte peak during preparation. The fixture, stage,
index candidate and cached fingerprints are constructed before profiling.

## Decision

Add `Snapshot::schema_refs()`, an exact-size, double-ended, fused iterator over
borrowed schemas in ascending live table-ID order. Deleted IDs leave gaps;
recreating a table name receives a new ID and occupies its new position. The
iterator lifetime is tied to its immutable snapshot borrow. No raw pointers,
unsafe, owned metadata or new serialization are required.

Add `table_count()` for operations needing only the current live table count.
Keep `schemas()` as an explicitly owned convenience, implemented by cloning the
borrowed iterator. Existing callers retain the original ownership and ordering.
This is an additive Rust API change, with no HTTP/file/cache/WAL change.

Preparation and complete state validation borrow every schema and retain all
root count/ownership/predecessor/transaction/type/coverage checks. Root selection
and the state fingerprint still use the original sorted IDs and exact bytes.
Removing schema copies does not bypass physical or derived-index validation.

Project status reads the count directly after its existing ownership checks.
Cache warmup needs to mutate its snapshot while loading indexes, so it retains
only bounded table names: at most 128 names of at most 63 bytes. It keeps the
original table order, file/directory checks, independent optional-file fallback,
whole-database input budget and diagnostic counts. This path is not allocation
free; it avoids copying every column to cross a mutable borrow.

## Evidence and limits

The original warmed native preparation samples for 1/64/128 wide tables now
request 792/11,184/22,192 bytes, with peaks 656/2,480/4,784. Deleting the prepared
state releases every tracked allocation. A separate 1000-pass borrowed scan of
all 128 schemas allocates no heap bytes or blocks. Exact dimensions and source
hashes are recorded in the [observation](../measurements/2026-10-07-borrowed-schema-inventory/operation-allocations.json).
These figures exclude cold fixture/stage creation, allocator overhead, stacks
and profiler bookkeeping. They do not establish throughput, RSS, cold memory,
retained-model admission or a whole-process quota.

Tests exercise ID gaps/recreation, mixed front/back iteration, exact size and
exhaustion, pointer identity, owned-copy independence, old snapshots, concurrent
readers, complete 128-by-64 metadata, refused final-table changes, unchanged roots
and replay fingerprints. Both managed WAL versions keep metadata through failed
transactions, reopen and backup restore. Wide cache warmup preserves corrupt
optional files and WAL bytes. Bounded fuzzing compares an independent catalog
through creation/drop, duplicate/missing refusals, COW row mutation, discarded
views and full physical replay.

Numeric model/cache/staging/transient budgets, the combined durable writer and
production acceptance gates remain open. The experimental model is still an
in-memory mechanism and supplies no durable acknowledgment.
