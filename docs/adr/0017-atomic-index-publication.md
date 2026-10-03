# ADR 0017: standalone atomic index snapshot publication

Status: implemented/tested on Linux local filesystems; table/WAL integration pending.

## Decision

`IndexStore` owns a private directory through an exclusive advisory directory
lock. Its selected `tree.ebif` is a regular single-link mode-0600 file; the
directory is mode 0700. Opening validates exact bounded EBIF bytes and the entire
tree. It never adopts staging files or falls back from a damaged selected file.

Creation validates the tree before filesystem mutation, writes/syncs an EBIF-1
snapshot inside a private staged directory, verifies the stored bytes, syncs that
directory, publishes with no-replace rename and syncs its parent before return.
Existing files, directories and symlinks are preserved. Revision starts at one.

Replacement first validates a complete delta against the owned snapshot. It
rechecks private directory device/inode identity and compares the selected disk
snapshot with owned state. Changed/corrupt state poisons the owner. Then it
writes/syncs/verifies a private temporary file, atomically replaces `tree.ebif`,
syncs the owned directory and returns the new revision. The directory lock spans
active-file inode replacement. Pre-publication staging failures preserve the
selected state; post-rename sync failure reports unknown outcome and poisons the
owner until reopen. A lost response can correspond to a complete selected revision.

Expose explicit standalone developer CLI create/insert/get/delete/verify commands.
Pointers remain opaque; the CLI does not dereference table rows. The release
container probe executes these commands using its own synthetic volume.
Unit-only sync/kill callbacks are absent from production builds.

## Verification and limits

Nine real process-kill boundaries cover creation/replacement and returned ACKs.
Ten before/after sync-failure cases cover file, staged directory and publication
directory syncs. Two processes preserve all 20 snapshot increments. A 32-case
persistent model reopens after every generated operation. Path permissions,
symlinks/hard links, truncation/damage, oversized files, staging refusal, no-clobber,
stale deltas and ownership across inode replacement execute.

A failing test first reproduced a renamed root path redirecting writes into a
different same-shaped directory; explicit device/inode checks fixed it. This
protects detected accidental path replacement within the trusted-local-owner
model, not arbitrary racing filesystem control by a malicious same-UID operator.

This publisher rewrites an entire bounded snapshot. It is not an integrated index
WAL, a concurrent-reader/MVCC allocator or a logarithmic write-cost claim. The
relational engine still uses its original in-memory primary-key maps. Atomic row/
index root publication, record ownership/lifetime, longer table keys, wider I/O/
power-loss and production gates remain open. Existing database/WAL formats do not
change. See [ADR 0010](0010-index-maintenance.md) and [ADR 0016](0016-stable-index-snapshots.md).
