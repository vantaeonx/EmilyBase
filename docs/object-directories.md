# Native project object directories

`ProjectDirectory` is a synchronous filesystem owner for one project. It binds
the existing experimental [object envelope](object-format.md) to a retained
private directory and accepts typed object IDs rather than caller filenames.
This is trusted offline filesystem authority, not an authenticated HTTP service,
an AccountRoot integration or user row/file-policy authorization.

## Initialization and scope

The operator supplies an existing 0700 directory on the local Linux filesystem.
Initialize does not create parent directories, repair state or overwrite names.
It opens the final directory without following a symlink, checks private mode and
acquires an exclusive advisory lock. Cooperating openers refuse while it is owned.
Selected ancestors and filesystem administrators remain trusted.

Initialization publishes `.emilybase-objects` through the original owned byte
publisher. The marker is exactly one 96-byte EMILYOBJ v1 envelope with the expected
project, zero object ID and empty payload. It is synced with its parent before
success. Opening requires this existing exact marker and never initializes it.
The original marker descriptor remains held; every operation rereads it through
the retained directory and rejects replacement inode, foreign scope, corruption,
aliases, nonprivate mode and invalid lengths. Directory mode is checked again.

Pre-existing unmanaged entries are preserved, not accepted as a validated
inventory. A marker does not grant access to their contents; every requested
object must independently pass exact scope/identity/length/checksum admission.
The project marker is metadata, not a signature or secret. A same-UID writer is
trusted and can still manufacture new valid images.

## Object operations and ownership

Names are derived solely as 32 lowercase object-ID digits plus `.object`.
Zero and all-ones IDs remain valid; their names cannot collide with the marker or
escape the directory. `put` bounds payload before storage work and publishes once
using the retained directory handle, file fsync, no-replace rename and parent
fsync. It reads back the complete selected image and rechecks scope before success.
Failure after selection reports an uncertain result requiring explicit inspection.
Existing object names are immutable through this API; there is no implicit retry.

`get` opens through that same descriptor with final no-follow/nonblocking flags,
verifies private regular single-link admission and complete expected object bytes,
then rechecks scope. It returns owned verified bytes with payload-redacted Debug.
There is no user-supplied relative path. Moving the directory and placing another
directory at its former pathname does not redirect existing-owner operations.
They continue addressing the original inode. A new opener addresses its explicit
path and must independently verify the requested project marker.

The lock guard explicitly unlocks on drop, including failed construction paths.
This prevents an incidental duplicated/inherited open-file description from
extending the authorization lifetime after the owner ends. No guard/descriptor is
exposed for cloning. Advisory locks are cooperation, not protection from hostile
filesystem administrators. Current checks are snapshots, not a general filesystem
transaction or protection against a same-UID writer acting after inspection.

## Offline CLI

Use `emilybase object-directory PATH PROJECT init`, then
`emilybase object-directory PATH PROJECT put OBJECT` with redirected binary stdin.
The exact stream, including NUL/invalid UTF-8/newlines, is preserved. Empty input
is allowed; more than 8 MiB refuses. Input is bounded and buffered before acquiring
the namespace lock. Terminal stdin is refused. Project and object text are checked
before waiting for input. The directory must already be private and initialized.

`emilybase object-directory PATH PROJECT inspect OBJECT` prints only verified
format/byte-count/SHA-256 JSON. It never prints payload or repairs input. Init
prints format/project metadata only. Stdout failure may follow a completed
initialization or object publication: inspect the directory explicitly rather
than assuming rollback. Repeating init/put never replaces the selected name.

## Limits and evidence

Tests cover independent projects with equal IDs, moved paths, marker substitution,
current permissions, aliases, corrupt/oversized images, immutable generated
histories, exclusive openers, binary stdin and failed stdout. Received init/empty/
full object results are killed and reopened; these do not simulate power loss.
The inherited-description lock regression fails before the explicit-release fix.
See [ADR0118](adr/0118-retained-project-object-directories.md).

Project roster/AccountRoot integration, HTTP uploads/downloads, user file policies,
inventory/delete/staging cleanup, quotas, signed URLs and object backup/restore
remain open. Current root backups do not include these directories. Larger
streaming files, hostile-filesystem guarantees and production acceptance are
separate work. Do not attach real application data.
