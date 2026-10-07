# Retained index buffer capacity

The original B+ tree now removes spare owned payload capacity before retaining an
immutable page. [ADR 0056](adr/0056-compact-retained-index-buffers.md) records the
reproduction, publication boundary and limits.

## Shape and ownership

Validated text keys retain exactly their UTF-8 byte length. Key, leaf-pointer and
branch-child vectors retain exactly their element count. Empty leaf vectors retain
zero elements. Owned boxed conversion establishes that reported shape; already
exact buffers keep their addresses. Caller capacities carry no stored meaning.

Constructor/decode validation precedes normalization. Internal changed-page and
new-page publication use the same helper. Equal pages keep the same shared owner;
old snapshots and their borrowed keys remain immutable. Both dense and stable ID
policies retain their ordering, split/rotation/merge/root-collapse behavior. Sparse
retired IDs can be reused without reusing an old retained page body.

EBIX/EBIF bytes, links, CRCs, version-1 frozen fixtures and snapshot fingerprints are
unchanged. The native standalone publisher still stores whole snapshots separately
from table/WAL transactions. Neither a new byte-admission setting nor a durable
index acknowledgement boundary is enabled.

## Reproduction and native sample

Before implementation, three constructor/insertion shape regressions and the native
guard failed. One three-byte key with 1048576 incoming capacity retained 1049016
requested bytes after insertion. The same isolated sample now retains 323 bytes,
peak 1048928, and zero current bytes after tree release. Separate leaf and branch
samples retain 43 bytes each, peaks 1089696 and 1081344 respectively, and return to
zero after page release. Inputs and oversized input vectors are constructed inside
the measured region; the empty tree/old empty view precede profiling.

Counters are observations of release builds using the optional dhat allocator,
not RSS, allocator usable-size or a whole-model quota. Allocator rounding/overhead,
stacks and profiler bookkeeping are excluded. Diagnostic output occurs after
profiler shutdown. See [source-bound observations](measurements/2026-10-07-retained-index/operation-peaks.json).

Private tests inspect actual vectors rather than cloned range results, because
cloning text can silently hide spare-capacity retention. Public checks similarly
inspect borrowed keys before round-trip reconstruction. Independent generated
mutation histories keep up to four historical views and compare complete rows,
exact images, owner identity and shape. Full capacity, all branch arities and
standalone persistence/reopen are exercised; short sanitizer campaigns add
bounded adversarial sequences but do not establish exhaustive coverage.

The caller may already own arbitrarily large buffers before calling this API.
Compaction can temporarily coexist with them. Map/Arc/cache overhead and model,
staging, replay and worker transients still require separate numeric accounting.
This retained-shape fix provides a prerequisite for that work and does not change
production-readiness or format-upgrade acceptance gates.
