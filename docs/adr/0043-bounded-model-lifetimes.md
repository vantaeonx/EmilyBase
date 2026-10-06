# ADR 0043: reserve experimental model lifetimes

Status: accepted for the memory prototype. Numeric heap and durable/server
admission remain open.

## Context

Sharing tables, keys and row bodies reduces unnecessary copying, but does not
prevent a caller from retaining arbitrarily many historical generations or
preparing states for several databases at once. Encoded lengths and measured
allocation peaks describe different resources. Neither gives an allocator quota.

## Decision

Add an explicit `ModelPool` around the existing synchronous memory model. Its
validated, immutable limits cover registered database identities, distinct live
or pending generations, owned reader objects and active writers. They are counts,
not bytes. Projects/generations/readers/writers have respective hard configuration
bounds of 128/4096/4096/4. Zero readers or writers disables that operation; at
least one project and one generation slot are required. No automatic defaults
pretend to select a safe deployment size.

One mutex protects small reservation decisions only. Before constructing the
empty model, create reserves its project and initial generation. Before cloning
staging metadata, begin reserves a writer and a future generation atomically.
Only one writer can be active for a database, including a prepared operation.
Different projects share the global writer/generation limits. No user callback,
storage operation, index construction or state destruction executes in the ledger
critical section. Poisoned public operations refuse with a typed error; private
non-cloneable leases still release during unwinding.

A generation owns both a complete Model and its lease. It releases actual state
before releasing its slot. Current state, readers and staged bases share that
same generation. Multiple readers of one state consume reader slots but one
generation slot. A retained old reader keeps its generation charged after a new
state is published. Drop, rollback, prepare failure and refused publication
release their reservations. A prepared future generation retains its writer
until publication/drop; admission does not become free merely because preparation
has finished.

Registration stays pinned by every descendant even after the publication owner
is dropped. A same-ID create therefore cannot reuse the namespace until the last
reader/stage/prepared state disappears. Publication checks exact generation
instance identity, then the original state fingerprint. Equal fingerprints and
IDs from another pool do not authorize publication.

The wrapper constructs its own models; it cannot import an already cloned raw
Model. Read/stage/prepared APIs expose borrowed rows/schema and copyable identity,
location/root/component metadata, but never a clonable Model/Snapshot/Selection.
Explicit reader cloning is fallible and admitted. Automatic primary-tree rebuilding
uses the original arena and exact predecessor, transaction, key type, current
coverage and physical-image validation. Any rebuild error aborts the whole stage.
The raw experimental API remains available independently and is outside this pool.

## Consequences and verification

Deterministic tests cover exact bounds, no partial reservations, disabled access,
old readers, clone refusal, descendant-pinned namespace, errors, rollback, prepared
lifetime and foreign-pool/project publication. Thread barriers hold winners while
checking the shared ledger: eight projects admit exactly four writers; eight
contending callers admit one project writer and four reader objects. An independent
32-case sequence model predicts distinct generations, pending reservations and
refusals while verifying historical/current values. Weak references confirm actual
state release; an injected internal panic checks fail-closed poison and cleanup.

No heap-byte upper bound is claimed. Map/arena capacity, lazy read caches, temporary
validation buffers, shared Arc references, caller-owned row/schema copies, OS
threads and raw models require separate accounting. This prototype does not
reserve HTTP workers, replay, backup or durable WAL buffers and writes no files.
Existing runtime file formats, hashes, transaction acknowledgment and network API
remain unchanged. Full numeric reservation and durable index/WAL participation
remain prerequisites before integrating this pool with the server.
