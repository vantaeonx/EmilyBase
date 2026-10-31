# Native authenticated self-owned file publication

[ADR0177](adr/0177-native-user-file-uploads.md) adds the synchronous library method
`AccountRoot::public_publish_file(project, access, now, AccountFileUpload)`.
This is not an HTTP/SDK upload endpoint. `now` is trusted service time.

## Input and authority

AccountFileUpload borrows name/payload and accepts typed FileId/ObjectId names.
It cannot specify ownership. The original user-file frame requires actual
current admission, session and installed policy, derives the native database
identity under its original owners and keeps the genuine FilePolicyPrincipal
borrowed throughout execution. Creation authorizes SelfOwned against that
principal's actual account ID. The service key, refresh credential, foreign
project's access credential or an earlier receipt cannot substitute for access.

Create denial precedes native name/size, collision and quota observations.
Allowed creation still requires the native bounds: name is nonempty, at most256
UTF-8 bytes and has no control characters; payload is at most8MiB and must fit
persisted operator capacity. The display name is metadata, never a path. Object
paths are formed exclusively from canonical typed IDs. Slash/dot text in a display
name does not select filesystem locations. Users cannot change quota through this
method. Physical objects, including unreferenced or logically removed objects,
remain charged. Existing logical/physical identities return typed native errors;
creation never overwrites. Once create is authorized, these errors can disclose
collisions within that project. This API does not promise collision concealment
between authorized creators of the same project.

Read, create, rename and remove rules are independent. ReadDeny can coexist with
SelfOwned creation. Only AccountFileWrite escapes: logical ID, actual commit
revision and previous_revision zero, meaning the reference was absent for this
creation. Neither FileInfo nor any owner, physical identifier, display name,
payload bytes/length/digest is returned. Request Debug reports only payload size;
receipt Debug reports revisions. Neither type serializes credentials or acts as
a future-call grant. Zero is not proof of historical non-use or an idempotent
retry contract: the native engine permits removed logical IDs to be reused.

## Durable publication and failure

The native engine verifies the original graph, admits physical quota, publishes
and synchronizes the immutable blob, retains its actual selected descriptor,
then commits the logical reference through its own WAL. Original common, project,
private-WAL and native selections remain checked. The private proof's borrow
prevents legitimate concurrent session/policy mutation through that owner.

An accepted BeforeBlob can still be followed by ordinary quota/object-collision
refusal without a new object. Such refusal leaves the healthy root usable and
preserves exact histories at fixed service time. BeforeCommit already has a
published blob: failure there leaves a charged orphan without a logical row.
After metadata commit, late refusal can preserve the complete reference even
though the caller receives no receipt. Native OutcomeUnknown or any final outer
failure after possible publication returns AccountFileWriteUnknown and closes
the entire Root. Later calls refuse with AccountFileWritePoisoned. Whole-frame
trusted unwind closes it after an admitted blob attempt or possible publication.
Do not automatically retry, remove objects, compensate or roll back; close,
inspect and explicitly reopen. Even an object-only orphan can prevent replay of
the same object ID, and this is intentional.

Forward authentication may append exactly one private clock transaction before
a later create denial. The native metadata WAL remains untouched on that denial.
Closed admission refuses before observing time. Backward observations refuse
after restart. No implicit migration, provisioning or permissive policy exists.
Verified independent restore preserves actual file owners/policies but revokes
copied sessions and closes public admission; explicit reopening plus a fresh
session is required for a new upload.

## Verification and limits

The [source-bound verification](measurements/2026-10-31-native-user-file-uploads/verification.json)
records actual commands, exits and frozen inputs. Tests cover genuine users and
projects, independent ReadDeny, key rotation, denial ordering, normal collisions
and quota, byte bounds, private clock behavior, current session revocation,
original descriptors, substituted sources, trusted unwind, actual signal9 kills,
charged orphans, generated decisions and independent restore on both WALs.

Process termination is not a power-cut test. Source checks are finite observations,
not a namespace lease against a malicious host. HTTP/SDK file methods, signed
URLs, reclamation, roles, realtime, dashboard, Kotlin, aggregate resource admission
and every unmet deployment/release/security gate remain open. No real user data
is involved. Existing source evidence for unchanged auth/native internals is
separate from the fresh adapter test totals.
