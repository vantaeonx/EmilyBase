# ADR0176: current-user native rename and removal

Status: accepted, 2026-10-30. Scope: synchronous AccountRoot library.

## Decision

Connect the selected-reference native checks from ADR0175 to the existing
AccountRoot user-file frame. `public_rename_file` and `public_remove_file` take
access credentials and trusted service time; neither accepts a service key or
caller-asserted owner. Current admission, a genuine session and installed policy
are required before native lookup/name validation. The actual native database
identity is derived under the original common/private/native owners.

The installed borrowed FilePolicyPrincipal consumes Selected's actual owner.
Rename/remove rules remain independent from read. Authorization precedes CAS and
no-op acceptance; missing and unauthorized references return Denied. Authorized
stale revisions retain native Conflict behavior. No implicit migration, project
provisioning or permissive default is introduced.

Return only AccountFileWrite: logical ID, resulting revision and previous logical
revision. A verified rename no-op has equal revisions and writes no native WAL.
Removal returns its actual commit revision. The receipt contains no owner,
immutable object ID, display name, payload length/digest or permission. Native
FileInfo/FileRemoval remain internal to this adapter so ReadDeny cannot be
bypassed through a write result.

## Durability and failure

Check original common/private/project selection and current proof at actual
native callbacks. Track possible writes only after accepting BeforeCommit or
observing native OutcomeUnknown/Committed. Preserve outer typed guard errors.
Any later error, final common/private failure or trusted unwind closes the
whole AccountRoot and withholds the receipt. Do not retry, delete, repair or
roll back possibly committed data. Close, inspect and explicitly reopen.

A healthy selection denial, invalid input, CAS conflict or verified no-op does
not invent a native write. Forward authentication time may still append the
existing private clock before a later denial; that is separate from a file
mutation. Metadata removal does not delete or uncharge physical objects.

## Boundaries

This adds actual current-user metadata edits only. User upload/owner derivation,
HTTP/SDK file methods, signed URLs, reclamation, roles, realtime, deployment
drills and unmet release/security gates remain open. No format or schema changes
are introduced. Tests use genuine sessions and synthetic data on both WALs,
including actual process termination and independent verified restore.
