# Native project-service file mutations

Experimental synchronous backend Rust API. The current project service key is
operator/backend authority. This is not a user file policy, anonymous upload,
role assignment, signed URL, new HTTP route or SDK file API.

AccountRoot now offers publish_project_file, rename_project_file,
remove_project_file, project_file_quota and set_project_file_quota. Publish takes
borrowed ProjectFileUpload input. Its owner is native metadata, not a principal
assertion. The input has no secret/payload/name serialization; Debug reports only
length. Returned native metadata is a copied observation, not future permission.

## Current authority and actual owner lifetimes

Reuse the existing readonly component frame without weakening its read semantics:
ready, current service key, exact v2 file binding, per-call original project
metadata descriptor/bytes/stamps, original common/private/registry/file topology,
and final full readiness after native borrows end. Deny v1/unbound/foreign/old keys.
No detached permission receipt or reopened source native component is introduced.

Repeat that same common/key check in every actual NativeWritePhase, using
[native guards](native-file-write-guards.md). Native selection checks enclose the
callback. The selected new/existing/all-quota payload descriptors retain their
existing lifetimes across actual commit checks. Original common/private/native
owners remain held; the operation does not acquire an unrelated public data writer.

The ProjectFileKey descriptor remains selected through the enclosing common proof.
The native payload readers span their own actual guarded phases; the API does not
promise to export or retain them after the native method returns. Arbitrary
privileged host writes between checks remain outside a namespace-lease guarantee.
Private/public WAL privacy checks are not a lifetime physical-byte lease for
unrelated sources. Backup retains its separately documented stronger source proof.

Current key admission precedes native input/CAS/large payload work. Native bounds,
revision/scope CAS, name validation and physical quota/orphan accounting remain in
the native store. Quota receipts cannot be used across project/database scopes.
Logical removal preserves immutable physical objects and their quota charge.

## Unknown outcomes close the whole live root

Track possible native publication when a publish reaches BeforeCommit (its blob
is already durable), when any operation reaches Committed, or when native reports
OutcomeUnknown before either callback. Preserve the original typed common/key
failure when converting its callback refusal to native GuardRejected.

If any enclosing operation fails after possible publication, return
AccountFileWriteUnknown and set an in-memory whole-root poison flag. This also
applies when native returned success but the final common/key proof then failed.
Every subsequent actual root operation checks this flag through ready and returns
AccountFileWritePoisoned. Restoring external metadata alone does not resume the
partially observed root. Explicitly close, inspect and reopen; never automatically
retry, delete a physical orphan or undo confirmed metadata.

Before publication, return an ordinary typed error. Staged metadata pre-commit
refusal retains the existing conservative native poison/reopen rule without
claiming a committed transaction. Name/quota no-ops do not invent publication;
an ordinary late common failure can remain usable after the original external
selection is explicitly restored and native data has not changed.

The poison flag is process-local and is not a new persisted format. Reopen uses
the original recovery rules and actual confirmed data. No partial copied receipt
is returned after a failed common/key proof. This does not create a distributed
transaction with unrelated private or public databases.

## Verification scope

The new cases exercise both WAL versions, all four real mutations, no-ops and
invalid/CAS inputs, current/foreign/rotated/v1 keys, original owner/descriptor
lifetimes, scoped metadata, unchanged unrelated histories, independent rebackup
and restore, and generated recovered-state models.

Late common-manifest and project-key metadata replacements are injected at every
reachable actual phase and after native return. Tests restore original metadata
and still require the whole-root poison after possible publication. Native
post-commit namespace failure preserves foreign bytes and confirmed metadata.
An actual 50-signal9 matrix covers both WALs, four operations, each reachable phase,
native return and actual API return, followed by recovery and a new independent write.

See the source-bound [verification report](measurements/2026-10-20-project-file-write/verification.json).
New parser/format/dependency, HTTP/SDK mutation, user-file policies, global admission,
container v2 recovery/upgrade and production acceptance are not supplied by this change.
