# Native authenticated user-file reads

The experimental synchronous AccountRoot library now offers public_list_files,
public_file_info and public_read_file. These methods require an actual current
access session, current explicit public admission and an installed file policy.
Their arguments contain no project service key. Existing native backend methods
continue to require the current service key and retain their separate authority.
No HTTP/SDK file route or user upload/rename/remove command is added in this step.
Subsequent [native user metadata edits](native-user-file-edits.md) connect
rename/removal to this same original current-user frame.

## Actual owned admission and current policy

The internal file frame selects the original project metadata descriptor before
user authentication. The metadata selection has no authority on its own. The service
wrapper still checks its key; the user wrapper uses an internal owned project and
checks current admission/session. A retained original private WAL descriptor is
established against the actual AccountStore before any forward clock append.
Authentication precedes selection of the requested native file binding. Invalid
project IDs, missing private ownership, closed/absent admission and invalid sessions
refuse without implicit migration or provisioning.

Inside the same original private/native/common frame, derive the actual native
database ID, recheck admission and obtain the currently installed borrowed
FilePolicyPrincipal with the genuine current session. Check its exact context before
missing-reference or empty-list work. No earlier copied identity or receipt grants
this operation. The private borrow excludes credential/admission/policy mutation
through that store during consumption. The public_* methods require authenticated
user authority; they do not expose internal owned-project or metadata capabilities.

The original common tree/WAL metadata, project metadata selection, registry directory
and private WAL selection remain checked through final output handling. Actual
native operations and retained readers verify their own original selection and
content. The original private WAL descriptor spans forward authentication clock
appends and final enclosing checks without another writer/directory lock or a
component reopen. These are finite checks, not a filesystem namespace lease.

## Visible metadata and complete payloads

List returns only committed references permitted by the installed read rule:
owner, authenticated or deny. Metadata returns None for a missing or hidden
reference. Context/authentication is still checked for an empty result. Physical
orphans are never references. Copied FileInfo/AccountFileRead values contain no
current authorization and cannot reserve access to a later operation.

Payload authorization uses the actual retained native reader's current owner,
before any payload copy is released. Missing and unauthorized payloads both return
ordinary denial. A requested maximum exceeding MAX_ACCOUNT_FILE_READ_BYTES or a
permitted payload larger than that requested maximum returns Limit. Empty permitted
payloads support a zero-byte bound. All metadata/native graph and payload caps remain
in force. These bounds are not a whole-process heap reservation or a timing-side-
channel guarantee.

Backend and user payload copies use the same native reader implementation. It
allocates a bounded complete result with checked reservation, verifies exact length,
checks the original reader and outer frame and only then releases the full owned
copy. It exposes no partial stream, unfiltered data handle or detached principal.
Denied/failed operations release their retained descriptions. Existing native source
poison/reopen rules remain strict after detected physical changes.

## Lifecycle, clock and restoration

Now is trusted service time, supplied by the native caller. A forward attempted
authentication can durably append its private clock even when credentials are
subsequently denied. The installed-policy verification reuses the same observation,
so the two checks append at most the one actual forward clock change. A closed
admission refuses before that attempt. Backward time is not silently clamped.
A held proof does not extend wall-clock expiry or authorize future calls.

Logout, refresh replacement, disable/reenable, password epoch changes and expiry
reject old access on the next operation. Closing admission denies all user-file
methods. Replacing a policy affects the next call without issuing a token. Service
key rotation is independent of genuine user-session authority. Verified common
restoration preserves installed policy/native binding and user IDs, revokes old copy
sessions and closes copied admission. An operator must explicitly open it before a
fresh copied session can read its permitted files. The source remains unchanged.

## Verification and remaining scope

Native integration cases use real distinct accounts, both WALs, actual authoritative
file ownership, no service-key fallback, exact clock commits, missing/empty/orphan
handling, bounded copies and independent restore. Current source checks include
48 metadata replacements, six payload replacements while the reader is retained,
12 bounded real workers for FIFO/symlink/hardlink/unsafe-mode changes and14 actual
signal9 kills. A twelve-case generated model compares current user visibility with
an independent native owner inventory.

The initial regression accepted an equal-byte project-metadata replacement at the
authenticated boundary because its original descriptor was acquired afterward.
The test failed before the factory moved ahead of authentication. The parser and
metadata encoding domain were not weakened. V1, undeclared file bindings and invalid
project paths refuse without creating components or upgrading private catalogs.

Final scope and frozen inputs are recorded in the [verification report](measurements/2026-10-21-user-file-reads/verification.json).
No file/schema/record version or dependency changes in the read increment. User uploads,
HTTP/SDK access, roles, signed URLs, realtime, dashboard, container v2 upgrade/recovery
and unmet production acceptance remain open. Synthetic data only.
