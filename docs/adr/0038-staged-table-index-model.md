# ADR 0038: complete staged table/index state model

Status: accepted for an experimental synchronous in-memory model only. It writes
no files/WAL and supplies no durable ACK. ADR 0031 remains proposed.

## Decision

Add a standalone `commit-model` crate that stages existing validated relational
events and complete stable primary-index snapshots together. An initialized model
has logical transaction one and no tables. Typed events stage only in a private
relational clone; drop discards it. Any failed event/index admission aborts that
stage, including previously successful changes. A stage admits at most 256 events
and 128 distinct index candidates, and refuses duplicate candidates and restaged
initial markers. Empty stages do not advance the logical transaction.

A changed live table requires a new selected index. Creation starts revision one;
later candidates require the exact prior root revision, owning transaction and
canonical IndexSnapshot fingerprint. New candidates belong to the target logical
transaction. Unchanged table roots can retain earlier transaction numbers. Drop
retires its root; a recreated name receives the new monotonic relational table ID.
Index-only maintenance can prepare a new complete selection without row events.

Preparation validates the stable arena/root/revision/counts, exact base and every
selected tree against all eligible current keys and their physical row images.
It checks schema key type and exact excluded long-text count. Missing/extra roots,
foreign table trees, obsolete pointers, wrong owner and incorrect counts are
refused even when headers/checksums are valid. Sparse arena IDs/root movement and
reuse remain valid only with the exact base and current live row coverage.

Prepared states are immutable and expose counts/rows/roots for inspection. Memory
publication compares an exact state fingerprint and swaps one complete Arc. The
fingerprint includes database ID, logical transaction, exact relational pages,
root count, sorted fixed root records and canonical full index fingerprints. It
refuses old prepared writers and equal-transaction divergent forks. Old cloned
views remain unchanged. The digest is public integrity, not a credential or MAC.

## Evidence and remaining gates

Focused cases exercise atomic memory publication, rollback/abort, complete two-
table namespaces, old views, exact-base rejection, schema/counts, drops/recreation,
long keys, arena split/merge/reuse, event limits and counter exhaustion. A 32-case
generated operation model compares both tables with an independent row map,
including late invalid pointers and exact state retention after refused plans.

This model reuses the original Snapshot's relational/physical validation and the
original index's topology checks; the reference row map is independent test data.
It does not independently reimplement those validators or prove durability.
There is no restore/import API, network route or stable migration in this model.

Current per-table image and event bounds do not select combined encoded/memory
budgets. Preparing a complete state performs full validation and hashing; this is
not a throughput claim. Worst-case staged candidate memory, full-capacity rebuild,
multi-worker budgets, typed retirement WAL records, one synced commit fence,
recovery/backup/migration and power-loss/security/production gates remain open.
Runtime WAL 1/2, managed state, index sidecars and all existing formats are unchanged.
