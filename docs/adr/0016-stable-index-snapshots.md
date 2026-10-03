# ADR 0016: opt-in stable index IDs, canonical snapshots and bound write sets

Status: implemented for the standalone index; file publication/table WAL pending.

## Context

The dense version-1 arena renumbers IDs after a merge. That contract must remain
byte compatible, but incremental durable work cannot rewrite unrelated nodes
solely to close allocation holes. Snapshots also need root/revision/count metadata
and validation of the complete tree.

## Decision

Keep `new`, `from_sorted` and `from_pages` in their original dense mode. Add
explicit stable constructors/import: surviving pages retain IDs after deletion,
while allocation reuses the lowest free ID in 1..1024. A merge retires its removed
sibling; root collapse selects the surviving child's existing ID. An empty tree
retains its final leaf ID. The bounded page map is the allocation source of truth.

Stable imports require strictly increasing embedded IDs, existing EBIX-1 page
checks and full topology/separator/leaf-chain validation. EBIX bytes do not change.
Dense callers never silently switch modes. A retired ID can later identify another
page, so future readers need pinned snapshots or an explicit lifetime protocol.

Add the canonical EBIF-1 envelope described in [index format](../index-format.md).
It binds a nonzero local revision, root, page/entry counts and ordered sparse pages.
Header/page CRCs detect damage; reserved bytes are zero. Exact length and global
bounds precede page allocation. Filesystem publication is a separate increment.

Represent changes as `SnapshotDelta`: exact base revision and SHA-256 fingerprint,
next revision, final root/count, sorted changed/new images and sorted retired IDs.
Applying a delta checks the base, revision, disjoint/canonical changes and whole
resulting topology before returning a new snapshot. The base remains immutable.
Wrong/partial/reordered/stale changes fail. Unchanged pages are omitted. An empty
delta may advance the local revision explicitly; overflow fails. The write set is
an in-memory API without a serialized managed-WAL record yet.

## Validation and limits

Actual arena exhaustion, stable survivor IDs, hole reuse, sparse root collapse,
exact snapshot round trips, every small-envelope cut/mutation and repaired-CRC
semantic damage execute. Deltas cover replacement, splits, merges, root changes,
wrong bases and forged final topology. Two 48-case properties and an ASan snapshot
target run. Frozen dense EBIX digests remain unchanged.

The arena still stages copies and is bounded at 1024 pages/10000 entries/256-byte
keys; snapshots are at most 4198400 bytes. SHA-256 binds stale base selection and
does not authenticate attacker-controlled write sets. Table key limits, record
ownership/lifetime, EBIX versus slotted EBPG images, transaction page caps and
atomic table/index root publication remain unresolved. See [ADR 0010](0010-index-maintenance.md).
No durable-index acceptance gate closes.
