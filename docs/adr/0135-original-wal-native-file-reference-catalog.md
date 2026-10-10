# ADR0135: original-WAL standalone native file reference catalog

Status: accepted for the standalone native operator library; not AccountRoot or HTTP.

## Context

Native scoped immutable objects have durable publication and verified standalone
archives. An object filename alone is neither a logical file reference nor user
authorization. Per-call limits are not persisted operator policy. Connecting a blob
to metadata requires retaining its actual inode and directory across the original
database commit. Two independent durable resources cannot be called one transaction.

## Decision

Add a nonempty synchronous `files` crate over the original transactions/catalog and
object-storage crates. It owns a separate managed metadata Database plus one native
ProjectDirectory. The trusted caller acquires metadata then object ownership. Exact
private schema version1 binds project/database ID, immutable persisted physical
quota, and bounded logical references with distinct FileId, object ID, caller owner
metadata, display name, exact length/hash and actual commit revision.

Initialize only pristine metadata and empty initialized objects. Open validates
exact schema, one scope, bounds, positive non-future revisions, distinct object
references, complete native inventory, all reference hashes and physical quota.
Unreferenced valid objects are invisible through catalog reads but consume quota.
Unknown/corrupt/missing/foreign state refuses without repair, adoption or deletion.

Publish validates before writing, then durably publishes a fresh immutable blob
under persisted limits. Its SelectedWrite retains the actual descriptor/owner while
complete receipt validation, the original-WAL reference commit and final graph/
receipt checks run. Add explicit verify_complete to that guard; older verify remains
exact-object-only. Post-selection errors report uncertainty and poison FileStore.
No attempt rolls back already-synced blobs or blindly repeats an uncertain commit.

Reads require a committed logical reference and return a native ObjectReader
borrowing FileStore. Owners remain retained. Display text never selects a path.
Owner fields and pure row inspection are metadata, not current account authority.

## Consequences and remaining gates

The native library now has actual original-engine durable references and persisted
physical quota rather than untrusted per-call limits. Blob-first ordering may leave
an invisible charged orphan. A lost response after a committed reference may leave
a visible reference on explicit subsequent inspection. This preserves the difference
between an uncommitted transaction and an unacknowledged client response.

The implementation repeatedly verifies bounded complete inventory; this is an
integrity foundation, not a throughput claim or whole-process reservation. Native
operator/ancestor trust remains; scope/inode observations are not atomic against
arbitrary same-user writes. A final inventory observation does not lease later
sibling names. Matching project/graph copies also do not establish unique ancestry.

Metadata edits/deletion, authoritative quota updates, request idempotency, orphan
reclamation, common backup/restore and an upgrade policy need further contracts.
Existing runtime formats, server/CLI behavior, Root manifests and bundles remain.
Current auth/admission/file policies, lock ordering against the actual root gateway,
coordinated restoration and resource admission must pass before user HTTP.
[ADR0127](0127-proposed-account-root-object-integration.md) remains proposed.
See [native contract and executed evidence](../native-file-catalog.md).
