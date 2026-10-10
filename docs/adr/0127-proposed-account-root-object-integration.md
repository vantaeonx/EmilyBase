# ADR0127: proposed AccountRoot object integration

Status: proposed; no schema, endpoint or runtime behavior is implemented here.

## Context

The original native object store now has immutable scoped publication, retained
file identity, complete streamed inspection, bounded writes and standalone verified
backup/restore. Current AccountRoot owns registry data and an explicit private
account roster. Its manifest and bundle do not contain object directories.
Per-call WriteLimits are not authoritative persisted service quotas. Existing row
policies authorize table operations, not files. Joining these components requires
a defined failure boundary before exposing user uploads.

The native filesystem selection and a private original-engine WAL commit are two
different durable operations. There is no atomic transaction covering them today.
Reopening a matching filename after publication also loses the selected inode's
identity, even when replacement bytes are equal; ADR0125 reproduces that failure.

## Proposed ownership and visibility

An explicitly versioned future root manifest would declare the exact object-store
roster. Normal open would retain each declared directory and verify project/scope
membership. Missing, foreign or undeclared stores must refuse attachment. Existing
roots would require an explicit offline migration; ordinary startup must not create
stores, adopt orphan stages, reset credentials or silently change a format.

Private original-engine metadata would bind a server-issued logical file identity
to an immutable blob identity, project, owner, exact length/hash and revision.
Deletion would first commit logical visibility revocation. Physical reclamation
would need a separately reviewed retained-owner protocol, including backup/read
lifetimes; a filename-only janitor is outside this proposal. The exact private
schema and new manifest/bundle version remain undecided.

Authority must come from the actual retained root, current registry membership,
current session/admission state and a separately defined file policy. Copied user
metadata and a client-supplied project ID grant nothing. Operator/service authority
must remain distinct from admitted user authority. User filenames would be bounded
display metadata only; fixed typed server-issued IDs would select path components.

The lock order must be established against the current owned public/private
gateway and tested before adoption. In particular, a new object lock must not
invert existing root/registry/data/private ownership. A borrowed authentication
callback cannot return a detached proof to use after its owners have been released.
Whether file operations need the public data owner is a design question, not an
assumption that permits bypassing current root authority.

## Proposed write boundary

1. Under the required owners, authenticate and admit the current policy, count,
   bytes, request input, worker and resource budget. Quota settings must be
   persisted operator-controlled state, never limits supplied by an untrusted
   caller. Existing immutable blobs, including unreferenced orphans, must count
   toward the physical storage budget.
2. Publish a fresh private immutable blob with the existing no-replace/file/parent
   synchronization rules. Keep its actual selected descriptor and owned directory.
3. Commit the exact logical reference and quota accounting through the original
   private WAL, rechecking current authority at the actual admitted boundary.
4. Before success, recheck the retained blob identity/bytes, root scope, committed
   metadata and required ownership. A failure after either durable operation is
   an uncertain result requiring explicit inspection; it is not an automatic retry.

Existing public put/put_bounded return reports and release the selected descriptor
when they finish. Calling either and then reopening its path around a later catalog
commit would not implement this contract. A future crate boundary needs an opaque
owned prepared/selected blob handle that cannot be forged, detached from scope or
accidentally used after authority ends. No such cross-catalog handle is added by
this ADR.

Later [ADR0132](0132-selected-native-object-owner-retention.md) implements one
native prerequisite: opaque selected-descriptor guards borrowing the original
directory owner through later caller work. It does not retain AccountRoot/current
user authority or integrate any catalog commit, schema, persisted quota or common
backup. This proposal remains open; its other acceptance requirements still apply.

Later [ADR0135](0135-original-wal-native-file-reference-catalog.md) implements a
standalone native original-WAL reference/physical-quota pair under retained object
selection. It remains outside AccountRoot/server/CLI and adds no current user file
authority, root schema/roster, common backup/restore or accepted root lock order.
Those integration requirements are still open.

A crash before the metadata commit may leave a valid private orphan. It must not
be user-visible through list/download, and must not disappear from quota accounting
or be automatically deleted. A committed reference must never authorize a missing,
corrupt or foreign blob. A lost response may already have committed the reference;
recovery/inspection and explicit request identity semantics must resolve that state
before a caller retries. The precise idempotency contract is still open.

## Proposed coordinated backup and restore

Root capture must acquire and retain every declared public/private/object owner
before its first acknowledged prefix. It must verify the complete declared roster,
catalog references, all immutable bytes and both directions of the catalog/blob
relationship. The format must explicitly preserve or reject unreferenced orphans;
silently omitting physical quota state is unacceptable. Encoded byte bounds do not
replace whole-process, retained-image or worker admission.

Restore would fully validate the entire graph into a fresh private root, revalidate
all selected descriptors/inventory and synchronize before no-replace publication.
Current private restore rules must revoke old sessions and close public admission
before selection. Signing material must have an explicit restore/rotation policy
before signed URLs are considered. Existing EMILYBND/EMILYOBK images must keep
their documented meaning and cannot silently claim coordinated coverage.

No user file endpoint should be enabled until the same runtime mode has an executed
coordinated backup, verified fresh restore and independent subsequent-write test.
Signed URLs require a separate purpose/expiry/current-key/revocation contract;
their cryptographic design is not settled here.

## Acceptance required before accepting this proposal

- Reproduce every before/after blob selection, metadata commit and response boundary
  with controlled process kills; recover acknowledged references and deny references
  from rolled-back commits. Cover lost/uncertain responses without blind retries.
- Prove orphan invisibility and physical quota charging after restart, including
  full limits, corrupt inventory, duplicate IDs and interrupted creation.
- Exercise actual concurrent operators/users, lock ordering and revocation while
  requests wait for bodies/workers. Deny cross-project IDs, path traversal, stale
  admission/session/policy decisions and same-byte inode substitution.
- Execute complete capture/restore with mixed data/accounts/objects, missing and
  foreign members, corrupted lengths/hashes, near-capacity images and independent
  writes after restore. Verify old credentials fail from the first selected state.
- Run both supported Rust toolchains, meaningful generated models, critical-format
  sanitizer fuzzing and explicit resource admission checks on the final source.
- Choose and document the exact schema/version migration and rollback rules before
  changing decoders. Keep earlier readers' refusal behavior and original format
  fixtures; do not infer compatibility from a successful current build.

## Alternatives and consequences

Storing every file body as ordinary rows would bypass the immutable object
foundation and require a separate size/replay/backup analysis. Publishing metadata
first would expose a durable missing-blob interval. Treating independently captured
archives as coordinated would not establish a common source boundary. These are
not accepted shortcuts.

Blob-first ordering tolerates private orphans and needs explicit accounting and
operator recovery. It does not provide a cross-resource atomic transaction. The
proposal remains open until implementation evidence resolves schema, ownership,
quota, backup and resource questions. No platform stage or production gate closes.

Related: [retained root](../retained-account-root.md),
[user gateway](../user-row-enforcement.md), [object limits](../object-write-limits.md),
[selected identity](../object-publication-identity.md),
[root restore](../account-root-restore.md), [release scope](../release-readiness.md).
