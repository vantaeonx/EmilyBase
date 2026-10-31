# ADR0177: current-user self-owned native uploads

Status: accepted, 2026-10-31. Scope: synchronous AccountRoot library.

## Decision

Add `public_publish_file(project, access, trusted_now, AccountFileUpload)` to
complete native current-user file creation. The borrowed input contains typed
logical/object IDs, display name and bounded payload; it has no owner field.
Derive owner exclusively from the genuine currently borrowed FilePolicyPrincipal.
Require explicit admission and an installed create SelfOwned rule bound to the
actual selected native database. Deny before native argument, collision or quota
observations. Repeat original common/private/project checks and current proof
checks at the real native write boundaries. There is no service-key fallback,
implicit provisioning, migration, policy installation or admission opening.

Use the existing immutable object-first, own-WAL reference-second publication.
Preserve its bounded physical inventory and persisted operator quota, including
orphans. Never overwrite a logical reference or physical object. A failed create
must not acquire rename/remove authority. Read remains independent from create.

Return the existing minimal AccountFileWrite, with actual commit revision and
previous_revision zero for absence at creation. Zero is not historical uniqueness
or an idempotency token: a removed logical ID can be reused with a fresh object.
The receipt contains neither owner, physical ID, name, payload nor digest.

## Two failure states

An accepted BeforeBlob callback only admits the next object attempt. An ordinary
quota or object collision can still return without publishing anything. Clear
that attempt flag on an ordinary native error; do not invent a whole-root write
or poison a healthy root merely because this callback ran.

BeforeCommit already follows publication of the actual selected immutable blob.
Mark publication possible before checking that callback. Native OutcomeUnknown,
success followed by final common/private refusal, or any later trusted unwind
withholds the receipt and closes the whole AccountRoot. Catch unwind around the
entire user frame, including final outer checks. A panic after accepting the blob
attempt also closes the root, even when no later callback was reached. Native
callback unwind independently poisons its narrower native owner.

Preserve possible blobs/commits and their physical charge. No automatic retry,
cleanup, compensation or rollback is added. Close, inspect and explicitly reopen.
A forward authentication clock append remains a separate private transaction,
including when create is later denied. Caller time is a trusted service input,
not a user-controlled request value or renewable permission.

## Boundaries

No stored format/schema/dependency changes. Only synchronous native library
creation is added. HTTP/SDK file methods, signed URLs, reclamation, roles,
aggregate admission and every unmet release/security/container-v2 gate remain
open. Finite source checks are not a filesystem lease against a malicious host.
Signal9 tests establish process-crash behavior at real callbacks; they do not
simulate power loss. Synthetic distinct users/projects, both supported WALs,
current revocation, retained descriptors and independent restore must be tested.
