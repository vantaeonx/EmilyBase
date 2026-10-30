# Native authenticated file metadata edits

[ADR0176](adr/0176-native-user-file-edits.md) connects current user authority to
`AccountRoot::public_rename_file(project, access, now, id, expected_revision, name)`
and `public_remove_file(project, access, now, id, expected_revision)`.
They are synchronous library methods, not HTTP/SDK routes. `now` is trusted
service time. The service key cannot substitute for access credentials.

## Authority and results

The existing [user-file frame](native-user-file-reads.md) retains original project
metadata before authentication and the private WAL before forward clock updates.
It checks explicit current admission and access before selecting the requested
native binding. Under that same original frame it derives nativeDB and creates
the current installed FilePolicyPrincipal. No copied native metadata or earlier
policy decision is used as authority.

The actual mutation emits Selected(&FileInfo) before revision matching, no-op
acceptance or staging. The adapter checks the corresponding rename/remove rule
against that actual owner under the borrowed proof. Read, rename and remove are
independent rules: a user permitted to rename/remove need not be permitted to read.
Missing and policy-denied references return Denied. Authorized stale revisions
return Files(Conflict). Invalid names are rejected after user admission; no native
write occurs. Logical identity reuse selects the new actual reference under the
existing native contract, never an earlier copied owner.

AccountFileWrite exposes only id, revision and previous_revision. It is copied
result metadata, not a grant, session or retry capability. No display name,
owner/object identifier, payload bytes/length/digest or native FileInfo escapes
through this receipt. Debug reveals revisions only. A matching current rename
returns equal revisions after complete checks and preserves exact native WAL
bytes; a stale rename remains a conflict even if its text matches. Repeated
removal does not manufacture successful replay: the absent reference is Denied.

## Failure, retention and time

The guard checks common/private/project selection and proof context at native
boundaries. Selected uses the current installed action rule. Current credential
or policy changes cannot occur through the private owner while the proof is
borrowed. This is not an expiry extension or permission for a future call.

Before accepting BeforeCommit, failures have not admitted a native commit.
After that check succeeds, native durable I/O may occur even if the call errors
or unwinds before a Committed callback. Possible commit, native OutcomeUnknown,
late common/private failure and trusted unwind close the whole live Root. It
then returns AccountFileWriteUnknown or propagates unwind and refuses subsequent
operations with AccountFileWritePoisoned. No receipt is exposed until the original
native/common/private checks finish. Do not automatically retry, repair, delete
or roll back; inspect after close and explicitly reopen.

No-op failures do not claim a write merely because a result was prepared. Native
source corruption can still require reopen under the existing strict native
contract. Original descriptor checks are finite observations, not a filesystem
lease against a malicious operator. Immutable objects and orphan quota remain
after logical removal. User methods cannot set quotas.

Forward denied authentication may durably advance the existing private clock;
fixed-time tests distinguish that from native WAL changes. Closed admission
refuses before clock observation. No automatic migration, policy installation,
admission opening or project provisioning is performed.

## Verification and remaining work

The [source-bound verification](measurements/2026-10-30-native-user-file-edits/verification.json)
records actual stable/minimum commands and exits. Genuine distinct users/projects,
independent action rules under ReadDeny, current session lifecycle, exact CAS/no-op,
descriptor lifetime, source replacement, trusted unwind, real signal9 kills,
generated decisions and independent restore exercise the adapter. Existing user
reads and trusted backend operations remain covered by the native v2 live group.

User uploads, HTTP/SDK file access, signed URLs, roles, realtime, wider resource
admission, container-v2 drills and all unmet production/release gates remain open.
There is no new file/schema version or dependency change in this increment.
