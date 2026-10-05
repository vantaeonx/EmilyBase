# ADR 0039: combined experimental index image admission

Status: accepted for the in-memory model and existing snapshot hash allocation
path. This does not select WAL 3, enforce a heap/process quota or enable a durable
index writer. ADR 0031 remains proposed.

## Problem

The prototype bounded each arena to 1024 pages and candidate count to 128, but
did not bound their combined image count. Two 1024-page candidates followed by
another page could reach image validation before aggregate refusal. Independent
per-object limits do not form a combined admission budget.

Before adding the fix, two failing tests reproduce aggregate count admission above
2048 and the missing early candidate-budget refusal. They exercise complete valid
individual arenas and a late invalid revision, following an earlier staged row.
The row projection is deliberately unselected: complete live coverage validation
remains a separate prepare gate.

## Decision

Bound the model's accumulated candidates and complete selected state to 2048
live primary-index pages across at most 128 roots. Candidate admission checks a
checked running page count before allocating validation images or hashing the
candidate. Any refusal aborts the entire stage. Duplicate candidates still refuse
before charging another image set. Complete state preparation also checks the
combined count, including retained unchanged roots.

The bound is separate from 256 relational events, the per-table 1024-page arena
and all runtime WAL limits. It does not count sparse ID holes as images. It does
not authorize a candidate's identity, exact predecessor, shape or current rows;
the existing complete validators still run before publication.

### Capacity derivation

For a tree whose root is a branch, let L be its leaves, I its internal pages and
K its eligible entries. Every non-root leaf has at least seven entries. Every
non-root branch has at least eight children; the root has at least two. Therefore:

```text
K >= 7*L
I+L-1 = edge_count >= 2+8*(I-1)
I <= (L+5)/7
P = I+L <= 8*K/49+5/7
```

A leaf-root tree has P=1, including an empty selected tree. Across R roots both
cases give `P_total <= floor(8*K_total/49)+R`. Complete projection validation binds
eligible entries to unique current rows, so K_total<=10000 and R<=128. The resulting
loose maximum is 1760 pages. Selecting 2048 leaves headroom while preserving every
currently valid complete state. Temporary invalid candidates need not satisfy
the row bound, so their separate aggregate admission is still necessary.

If entry/occupancy/root limits change, revisit this derivation and its tests before
changing admission. The formula does not prove how much Rust heap those pages use.

### Encoded component inspection

`EncodedComponents` uses bounded u64 counts and checked arithmetic. Model and
Prepared reports measure existing history page images, full EBIF objects (one
header per root) and EBIR root bytes. They exclude database file headers, a future
WAL envelope/fence, retirement records, decoded state and retained views.

At 2048 pages/128 roots the full EBIF sum is 8912896 bytes; root metadata is 24576.
The loose report maximum also includes 65536 history images, for 277372928 bytes.
This arithmetic is not proof that the maximum is a reachable runtime database.
The model has no file/WAL writer, and runtime WAL capacity remains independent.

### Canonical hashes without a redundant full envelope

Encoding and fingerprinting share complete bounded image/topology admission and
one canonical header builder. Fingerprinting streams the header and images to
SHA-256 rather than allocating another complete EBIF Vec. Public snapshot
validation skips that final envelope too. Image vectors and reconstructed topology
still allocate; this is not an allocation-free or throughput claim.

An immutable Selection caches the fully validated canonical index fingerprint at
construction. Exact predecessor and state digest composition reuse it. Selection
owns its snapshot and exposes only immutable references, so publishing/retaining
views cannot stale the cache. A detached mutable snapshot clone affects no selection.
EBIX/EBIF bytes, hashes, delta bases, error admission and managed runtime formats
remain unchanged.

## Evidence and open work

Tests include independently constructed frozen EBIF hashes, direct full-encoding
hash comparison, sparse/full snapshots, invalid admission, generated mutations,
actual serialized component lengths and generated rollback/preparation states.
An independent minimum-occupancy forest builder exercises integer/256-byte UTF-8
keys with both branch grouping strategies. Full 10000-row/128-table publication
selects 1536 fragmented pages and checks every live pointer/value. A two-full-arena
stage accepts 2048 pages, then refuses the next page before invalid-revision
validation and preserves the original state. Four real threads retain exact old
rows, roots, pointers and component counts through 32 serial memory publications.

Encoded page admission is only part of increment 3. Input length reservation before
decoding, real retained/transient heap accounting, old-view lifetime control,
four-worker reservations, combined WAL bytes, cold replay/compaction and one synced
commit fence remain open. Security/load, physical-power-loss and production gates
remain open. No migration or real data is introduced.
