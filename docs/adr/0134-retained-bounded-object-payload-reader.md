# ADR0134: retained bounded native object payload reader

Status: accepted for the standalone synchronous native object library only.

## Context

StoredObject/get own a verified complete payload image. Report-only inspection
avoids that image but returns no cursor for subsequent native reads. Reopening a
filename for every chunk would lose the original checked inode. Retaining a raw
File alone would also lose the original directory-owner lifetime and scope checks.
Future file transports need a bounded internal reader before choosing HTTP policy.

## Decision

Add opaque ObjectReader borrowing the original ProjectDirectory and keeping the
actual readonly descriptor, expected report and original stable metadata. Admission
fully streams the existing envelope under exact project/ObjectId and rechecks the
original scope/metadata/visible inode. Its payload-only synchronous methods return
typed errors. Private fields prevent raw descriptor export, cloning or serialization.

Each read copies at most8192 caller-owned bytes with positional I/O and validates
owner/scope/private/stable metadata and inode before and after the copy, including
empty/EOF requests. A failed read clears its attempted prefix, preserves logical
position and irreversibly poisons this handle. Failed filesystem validation on
seek/verify has the same refusal state. Input-only invalid seeks are recoverable.
Payload-relative seeks may pass EOF but cannot access the native header or overflow.
Complete verify preserves logical position; consuming finish repeats full original
scope/hash/identity verification. Expected getters are not current authority.

## Consequences

There is no second complete payload image in the reader. Full admission and final
verification each scan the payload, and per-chunk scope checks add filesystem work;
this decision does not claim better throughput or zero total allocation. Independent
borrowed readers may run in scoped threads with separate positions while one native
owner excludes a second cooperating owner. Blocking work stays outside reactors.

The existing native trust boundary remains. These checks observe and refuse detected
filesystem change; they do not make arbitrary concurrent administrator writes atomic
or revoke chunks already consumed. Existing owned get is still useful when a caller
needs an immutable in-memory image. Formats/fsync/ACK/write APIs are unchanged.

No user/policy/signed-URL semantics or catalog/Root coordination is inferred. The
proposed AccountRoot schema/quotas/common backup gates remain open in
[ADR0127](0127-proposed-account-root-object-integration.md). See the
[reader contract and executed evidence](../object-payload-reader.md).
