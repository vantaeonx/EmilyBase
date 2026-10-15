# Retained native common-root observation

`NativeFileRoot::open(path, expected_project)` opens an existing standalone native
metadata/object pair. It retains the original parent/root, both actual children,
an original readonly WAL descriptor and the original scope-marker descriptor before
acquiring both independent engine/object owners. No raw engine, writable component
or directory descriptor is exported. This native API does not grant user authority.

```sh
emilybase file-root-info root 01010101010101010101010101010101
```

The CLI prints checked project/database identity, global revision as a decimal
string, physical quota and counts for objects, references, charged orphans and
payload bytes. It omits names, owner metadata, payloads and credentials. Relative
operator paths work; invalid project, wrong scope, a busy owner or invalid root
selection refuses without output or repair.

## Original selection and state

The retained parent must remain at its original visible path; the selected root
must remain its original private0700 inode. The root contains exactly `metadata`
and `objects`, with matching retained private child identities. Final leaf/parent
symlinks refuse; native ancestors remain trusted. Moving/replacing a root or child
does not redirect original owners into the replacement.

Original readonly WAL/scope files remain regular, private0400/0600, single-link,
bounded and selected by their original child entries. Size, inode and timestamps
are rechecked. The metadata engine must also confirm that the current child/WAL
belongs to its actual owner. The original object owner validates project scope,
every physical object and its complete inventory; the native catalog validates the
exact private schema, project/database binding, references and quota.

Opening, inspection, catalog listing and snapshot capture use original native
owners. Every observation compares the acknowledged WAL digest, quota, complete
catalog and physical inventory before/after work, while rechecking path selection.
The engine fully verifies the acknowledged prefix; the digest is a copied state
comparison, never backup-origin authentication. A detected read failure returns no
observation and poisons the wrapper until explicit reopen. It performs no repair,
uncertain write or recursive cleanup.

`list()` returns bounded native catalog metadata including display names. `capture()`
reuses the existing [coordinated capture](native-file-snapshot.md), which retains all
actual physical object descriptors through its own copy checks. The common wrapper
retains root/child/WAL/scope selection through its surrounding observation checks.
It does not extend physical object-descriptor lifetime beyond that original capture
or create a namespace lease. Returned immutable sensitive bytes remain separate from
durable archive publication and current user permission.

Validation is readonly. A recoverable abandoned WAL tail is left physically intact;
capture exports only the acknowledged prefix using original engine rules. Kernel
owner locks remain held throughout wrapper lifetime. Internal engine cache paths
borrow retained child descriptors; component owners drop before those descriptors.
No caller-owned descriptor lifetime is required.

## Scope and resource limits

This increment exposes observations and immutable capture, plus file-root-info.
Mutating common-root methods/CLI, current AccountRoot inclusion, authenticated file
routes, policies and signed URLs remain separate work. Standalone FileStore writes
retain their existing native-pair contract; they are not silently presented as
common-root guarded operations.

The WAL export/replay, bounded inventory/catalog state and immutable capture still
allocate. Root/child/WAL/scope/owner descriptors are additional retained resources.
Existing byte/count limits are not global memory/FD reservations or network deadlines.
Native Linux/proc availability, trusted ancestors and finite final observations remain.
No format/schema/ACK rule or dependency version changes. See
[ADR0145](adr/0145-retained-native-common-root-observations.md).

## Executed evidence

[Source-bound checks](measurements/2026-10-15-native-file-root-observation/verification.json)
exercise WAL1/2, empty/8193-byte payloads, exact persisted quota, a charged orphan,
metadata identity/history, both actual owner locks and descriptor-count return.
Immutable common snapshots restore independently with the same graph/state. Debug
and CLI reports omit private display names and payloads.

Forty-two substitutions span seven root/child/WAL/scope/mode/roster changes across
three owner-admission boundaries and both WAL versions. Ten original-parent moves/replacements independently span all five admission/read
boundaries on both WAL versions; neither original nor identical replacement WAL is
modified, and explicit separate reopen remains possible. Twenty-eight further changes
span before/after observation work. Detected changes refuse and poison the wrapper;
identical-byte replacement inodes do not substitute for originals. Wrong scope,
public root mode, extra entries and root/child symlinks preserve existing files.
An actual partial next-WAL record remains byte-for-byte intact across inspection,
listing and capture, while only original acknowledged state is returned.

Twelve new SIGKILL cases span metadata-owner-only, both owners admitted and caller
inspection/capture success, WAL1/2 and empty/8193-byte payloads. Source WAL/graph stay
unchanged; both owners can be reacquired after the process exits. Thirty-two generated
payload/charge/snapshot-restore models compare against independent expected state.
Actual CLI processes check exact count-only JSON, relative paths, both WAL versions,
wrong/malformed project, busy owner and unexpected-entry refusal. These are process
crash observations, not physical power-loss evidence or production acceptance.

A preliminary minimum-toolchain CLI run reproduced transient Busy when one test
forked while a peer fixture dropped its original engine owner. The focused test
passed alone; the paired two-thread executable reproduced the same failure on
attempt67. These two fixture/process tests now serialize inside their executable,
matching the native test-owner boundary.200 fresh two-thread executable runs
(400 cases) pass afterward. Engine/WAL lock semantics remain unchanged; failed
preliminary runs are excluded from the final source-bound matrix.

The final frozen618-source matrix passes626 checks per Rust1.99/1.89:301 native
files/storage/CLI and325 database/WAL/transactions/backup. Workspace/fuzz formatting,
strict all-target lint, affected builds and minimum fuzz builds pass. Twelve new
and143 prior native process kills pass per toolchain. All618 source hashes match
afterward. Parser/header/encoder/capture and lockfiles remain unchanged; no new
filesystem sanitizer, global resource or full-workspace acceptance is claimed.
