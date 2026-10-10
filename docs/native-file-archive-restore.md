# Common native file archive restore

`restore_file_archive(bytes, expected_project, fresh_operator_destination)` verifies
the entire immutable [paired archive](native-file-archive-format.md) before creating
any private stage. It restores original-engine metadata and complete native objects
inside one retained0700 root, then selects that root without replacement only after
complete readback, scope, physical quota and reference-graph checks.

## Owner and selection boundary

The original storage directory stage now supports explicit `at(parent_descriptor,
single_leaf)` creation. Original metadata `restore_bytes_at` and object
`restore_archive_at` use the retained native parent, preserving their component
fsync/readback/no-replace rules. Empty, dot, dot-dot, slash and NUL names refuse
before stage creation. An actual directory is required. Moving the original parent
does not adopt a different replacement at its old path. Existing path entry points
retain their parent-path checks; the native descriptor APIs grant no user authority.

The common restore retains both actual child-directory descriptors and requires
exactly `metadata` and `objects` under the original root. Restored Database and
ProjectDirectory owners remain locked; every physical object descriptor, including
orphans, and an independent original WAL descriptor remain held through selection.
Root/child modes and visible identities, actual WAL inode/bytes, project marker,
complete inventory and exact reference/quota metadata are rechecked before and after
the common root's original no-replace publication and parent fsync. Another directory
or identical-byte file cannot substitute for a retained original during these checks.

Original database identity, last acknowledged transaction, WAL1/2, file references,
quota and every charged physical orphan are preserved. Metadata checkpoint/cache
files are reconstructed using existing engine rules. No format/private-schema
version changes, rescoping or account permission is inferred.

## Failure and resource contract

Invalid whole-pair input or wrong expected project creates no stage/destination.
Before common selection, errors leave the common target absent and retain nonempty
private stages for operator inspection. Child-component uncertainty inside that
private root does not publish the common destination. Root-selection uncertainty
or any later error returns OutcomeUnknown and preserves the selected root. Existing
destinations refuse without overwriting their contents. No repair, recursive common
stage cleanup, uncertain retry or source changes occur.

This is a synchronous native Linux operator API. Source bytes, replayed metadata,
component buffers and at most128 physical object descriptors have existing bounds;
additional root/child/WAL/owner handles and allocations remain. This is no global
heap/FD reservation, network deadline, authenticated backup origin or namespace
lease. Native ancestor/operator trust remains. AccountRoot/user routes, policy,
signed URLs, physical power-loss evidence and production acceptance are still open.
See [ADR0141](adr/0141-owned-common-native-file-restore.md).

## Executed evidence

[Source-bound checks](measurements/2026-10-10-file-archive-restore/verification.json)
cover native WAL1/2, empty/8193-byte payloads, charged orphan and changed quota.
After source changes/removal, a common restored root preserves exact original WAL,
identity, transaction, references, physical charge and payload; independent writes
and later reopen succeed. No-replace refusal preserves later destination writes.
Thirty-two generated payload/removal/quota models compare common restore against
independent visibility, physical charge and exact acknowledged WAL state.

Thirty-two substitutions span both WAL versions, before/after root selection and
metadata/object child directories, same-byte WAL inode, in-place WAL corruption,
referenced/orphan object inodes, project marker and unexpected root entries. Prior
to selection they leave the target absent; after selection they report uncertainty.
The maximum128-empty-orphan case measures128 actual retained physical descriptors,
both resource locks and exact descriptor-count return after restore.

Twenty actual SIGKILL cases cover metadata-only staging, both components staged,
all owners admitted, durably-selected-before-result and caller-success, two WALs
and empty/8193-byte objects. Unselected pairs remain private; selected/acknowledged
roots independently reopen, preserve exact payload/physical charge/transaction and
allow the next metadata write. The preceding95 native kills rerun. Explicit native
descriptor entry tests separately exercise moved parents, invalid leaves and
no-replace behavior; prior path-based source/substitution/fsync checks also rerun.

The first integration failure was reproduced before changing implementation:
path-only component restoration normalized a descriptor path and returned ENOTDIR.
Explicit owned-parent APIs resolve it while preserving no-follow behavior. Failed
preliminary attempts and mutex-poison followers are excluded from final evidence.

The final frozen607-source matrix passes635 checks on each Rust1.99/1.89, including
36 additional account/AccountRoot/registry regressions. Workspace formatting, strict
all-target lint, affected builds and separate fuzz formatting/lint/minimum builds
complete. A bounded paired-decoder sanitizer campaign logs1,293,750 inputs in46s,
16 fresh synthetic seeds and444MiB final RSS under512MiB; no findings appear.
Its terminal Done marker is retained, while the numeric final process status was
not retained in polling. It does not fuzz filesystem restoration or certify maximum
archive memory use. Full-workspace acceptance remains source-specific and separate.
