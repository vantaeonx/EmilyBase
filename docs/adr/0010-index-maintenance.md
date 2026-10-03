# ADR 0010: Bounded index maintenance before durable integration

Status: standalone maintenance implemented and tested; durable integration pending.

## Context

An index usable by mutable tables needs replacement, deletion and a reliable
rebuild path as well as insertion. The initial version-1 arena imports/exports
dense IDs. Its 4096-byte `EBIX` images differ from the storage/WAL `EBPG` format.
Managed recovery currently accepts only append-only relational history.

## Decision

Implement original sibling rotations, adjacent leaf/internal merges, exact
separator refresh and root collapse. Stage deletion in a bounded tree copy;
publish only after full topology validation. Renumber reclaimed arena pages
densely, rewriting internal links and the root but never external row pointers.
This preserves the existing codec/import contract without adding a format version.
Index arena IDs are not stable public handles across deletion.

Replacement validates a cloned leaf and publishes that leaf only. It does not
rebalance or renumber pages. Errors, including missing keys, preserve prior images.

Build sorted unique entries bottom-up with balanced groups, including at leaf and
branch boundaries. Validate all keys, pointers, ordering and bounds before building;
validate the completed topology. The borrowed input is unchanged. At the current
entry cap, even maximum-size text keys produce a 768-page tree.

## Costs and durable integration boundary

Deletion clones and validates the bounded arena and may rewrite all page IDs;
it is not a logarithmic durable write set. Insertion also stages a bounded copy.
This is an experimental correctness baseline, not a throughput claim. A future
stable-ID allocator/free list and copy-on-write strategy need explicit tests.

Do not silently place raw index images into existing slotted records: an image
is 4096 bytes while a record is limited to 4058. Do not silently reinterpret WAL
pages: its current decoder validates a different page kind. A new durable design
must cover page kinds, allocator ownership, root/catalog publication and replay
of splits, merges and reclamation in the same transaction as row history.

Existing table text keys allow 3072 bytes, while this codec allows 256. Integration
must preserve existing accepted schemas/data or introduce an explicit versioned
policy; enabling this index cannot silently reject longer existing keys. Table
code must validate row pointer ownership/lifetime. A full 1024-page arena can
exceed the current 256-page normal WAL transaction bound; atomic publication
requires a compatible bounded write-set design rather than an unannounced increase.

## Validation and remaining gates

Deterministic tests cover both sibling directions at leaf/internal levels,
cascading merges, root collapse, complete deletion, actual capacity reclamation,
maximum UTF-8 keys, boundary sizes and exact reopen. Two 48-case properties compare
mixed CRUD and bulk/incremental construction with an independent ordered model.
Frozen digests from published d75751b preserve original version-1 images. Bounded
ASan fuzzing compares operation sequences with an independent model and exercises
raw/checksum-repaired images. These checks do not establish durable index recovery,
concurrent readers, power-loss behavior or production readiness.

Follow-up [ADR 0016](0016-stable-index-snapshots.md) implements opt-in stable IDs,
canonical envelopes and exact-base-bound in-memory write sets. Dense callers keep
this original contract; table/WAL publication remains pending.
