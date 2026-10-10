# Coordinated immutable native file snapshot

`FileStore::capture(&mut self)` returns one immutable FileSnapshot assembled under
the original metadata database and object directory owners. Its mutable borrow
excludes FileStore writes throughout capture. No source or destination is written.

After exact schema/scope/quota/reference/physical graph validation, retain and fully
verify every actual readonly object descriptor, including orphans, at most128.
Export the exact acknowledged original WAL, encode/replay the existing backup and
apply the same private metadata validator as live opening. Its quota/references
must match. Capture the expected complete object inventory. Before returning,
reverify every retained inode, descriptor-relative object scope, inventory and
exact original committed WAL. Capture descriptors close before return.

Post-admission failure returns no image and requires reopen. No cleanup, repair,
retry or uncertain destination exists. Same-byte object inode replacement and
same-inode source rewrites after copying refuse. Moving either original owner
directory retains the original descriptor source; capture does not adopt a different
replacement at its old pathname. Old top-level path selection is not certified.
Native operator/ancestor trust and finite final-observation boundaries remain.

FileSnapshot exposes immutable project/quota/references, original metadata backup
bytes/report and ObjectSnapshot. Every valid physical orphan stays in the image
and physical charge. Later source writes/removal cannot change these owned bytes.
Debug output omits contents, names, owners and database identity. Existing64 MiB WAL,
64 MiB physical payload,128 objects and8 MiB individual bounds remain. Headers,
replay, intermediate copies, cache and scratch also cost memory; this is not a
whole-process heap/FD budget, server admission or user authorization.

No original format/private-schema versions change and no new combined serialized
format is added. Persisting two independent images does not provide common atomic
publication. Existing component-restorer tests exercise graph preservation, not a
common restore crash guarantee. Combined format, common no-replace publication/
restore, their crash/fuzz campaigns and Root/user integration remain open.
See [ADR0138](adr/0138-coordinated-native-file-snapshot.md).

## Executed verification

[Source-bound checks](measurements/2026-10-10-file-snapshot/verification.json) cover
WAL1/2, empty images,8193-byte payload and charged logical-deletion orphan with
changed quota. Separate component round trips preserve old metadata, exact payload
and charge after source changes/removal, followed by an independent metadata write.
Twelve same-byte inode substitutions span two WALs, referenced/orphan objects and
three capture boundaries. Four moved-owner cases prove original-source retention;
four same-inode WAL/object rewrites refuse. Linux128-empty-orphan capture measures
128 retained descriptors at all boundaries, both locks and exact FD return.

Thirty-two generated mixed publish/remove/rename/quota histories compare snapshots
with an independent visibility/physical-payload model and reopen after every step.
Sixteen actual SIGKILL boundaries cover admitted, metadata-copied, objects-copied
and caller-success capture on both WALs with empty/8193-byte objects. Recovery
preserves exact acknowledged source state and allows fresh capture/independent
write. A snapshot lost with its process is not a persisted backup.

Final affected checks:578 on each Rust1.99/1.89, comprising257 native files/storage/
CLI and321 database/WAL/transactions/backup. All598 source hashes matched. Formatting,
strict workspace/fuzz lint, explicit builds and minimum fuzz builds passed. The16
new capture kills plus67 previous native kills passed per toolchain. No new sanitizer
campaign or complete current-workspace test result is implied. Lockfile changes add
only the local files-to-backup dependency edge, without external version changes.
