# Explicit offline account/file root restoration

`restore_account_file_archive_bytes_at(bytes, parent, leaf, pool, now)` verifies a
complete sensitive joined archive, prepares all declared components under one
original private root and selects that root without replacing an existing target.
This synchronous native Linux operator API returns copied AccountFileRootReport
metadata. It does not return a service owner or current user permission.

## Layout and preparation

The exact restored v2 layout is:

```text
root.json
registry/<project>/project.json
registry/<project>/data/...
private/<project>/...
files/<project>/metadata/...
files/<project>/objects/...
```

Require a trusted signed-domain timestamp, a private0700 parent including special
permission bits, and one1..255-byte leaf without slash/NUL/dot/dot-dot. Verify the
complete existing joined envelope, declared rosters, globally distinct database IDs,
private schemas and every file image before staging. Archive bytes remain unchanged.
The canonical v2 root manifest keeps the exact declared bindings and records `now`.

Keep the original staged root exclusively locked. Retain actual native root/private/
files directory descriptors and the original single-link0600 manifest file, never
reattaching a former pathname after a move. Prepare the public registry, then nest
all private account preparations, then all file preparations. Existing narrow
component protocols keep their actual original writers/readers through their work;
no component is closed/reopened to adopt its selected name before common publication.

Public data/API-key metadata and file catalog/payload/orphan images are preserved.
Private preparation gives every copied store a fresh session scope/time; legacy
versions1/2 become3, versions4/5 keep policies, and v5 closes public admission in its
reset transaction. Original source sessions/data remain unchanged. Restored old
credentials are invalid; independent new sign-in/data/file writes work after return.

## The final proof and owner lifetime

Before common selection, require exact top-level/private/files rosters, actual
original directory selections and original manifest inode/bytes/privacy/stamps.
While every component writer/payload reader is still held, capture independent
readonly descriptors for the complete prepared physical tree, including disposable
checkpoints, metadata and every orphan. Retain original directories/files, exact
rosters, physical identities/lengths/modes/link/time stamps and SHA256 content hashes.
Open children without following links or blocking on special files. Hash/read in
8192-byte chunks; do not retain complete extra file images.

This physical proof has separate caps:512 directories,1024 files,depth6 and384MiB
of actual files including preparation/checkpoint overhead. Source image/file-store/
object bounds still apply independently. These limits are not a whole-process
heap/FD reservation or a promise that every admitted input fits the host's resources.
The nested component chain is bounded by the existing128-project and4-file-store
rosters. Aggregate process admission remains open.

Repeat physical/roster/manifest proofs, sync private/files directories and manifest,
then use the existing storage no-replace publisher for root sync, rename and parent
sync. Every original component writer spans this common selection. Component final
proofs run while unwinding; inner writers are released in order. Do not claim all
component writers survive the last outer proof.

The common root lock and independent original physical readers survive that unwind.
Recheck the entire original tree and exact selected root/manifest, confirm parent
fsync, and repeat proofs before releasing those readers/lock and returning metadata.
A late appended tail, same-byte inode replacement, orphan change or namespace/
privacy change withholds the common receipt even after component writers finish.
Moving the original parent never adopts a foreign replacement at its old path.
These are finite observations under trusted native OS ownership, not namespace
leases, authenticated archive origin or a sandbox against administrators.

## Failures and runtime boundary

Before common selection, failure preserves nonempty private stages and does not
publish the outer target. A stage may contain individually selected prepared children.
After common selection, any observed nested/final failure is ComponentRestoreUnknown,
including uncertainty from the unchanged native publisher. Drop the unconfirmed
receipt; preserve selected/foreign files for inspection and never retry blindly or
promise rollback of acknowledged preparation. Existing file/directory/link targets
are never overwritten. Earlier component uncertainty remains typed even if the
outer root has not been selected.

Ordinary v1 AccountRoot/server startup still refuses this v2 layout. Native operators
can inspect/reopen the completed components independently after return; automatic
v2 live adoption, a joined restore CLI, current user-file authorization and production
acceptance are separate work. Existing v1 APIs and archive/WAL formats are unchanged.

## Verification

[ADR0161](adr/0161-offline-joined-root-restore.md) records the choice.
[Source-bound evidence](measurements/2026-10-16-joined-root-restore/verification.json)
records final commands, source hashes and exclusions. Nine new cases include empty,
one/four-project mixed WAL copies, original/new credential checks, preserved native
payloads/orphans, independent public/private/file writes, source immutability and
actual descriptor/lock lifetimes at preparation, selection and final unwind.

Seventy-two mutations span WAL1/2 and prepared/selected/finalizing boundaries, including
same-byte private WAL and manifest replacements, public unacknowledged tails, file
metadata/payload/orphan changes, private-directory substitution, modes, foreign
entries and hard links. Ten before/after actual sync faults distinguish private
stages from selected uncertainty. Invalid inputs/time/leaves, parent privacy and
occupied targets refuse safely; original parent moves preserve native authority.

Sixteen generated models vary roster size, WAL compaction and omitted file stores.
Depth6 succeeds/depth7 refuses; a sparse384MiB+1 file refuses without changing it.
Twelve actual signal9 kills cover empty and two-project sources at applicable registry/
private/file-ready, prepared, selected, finalizing and caller-received boundaries.
Copies/stages remain inspectable, locks release after kills, selected copies recover
fresh scopes, and independent retries work. The caller-received checkpoint lives in
the reused test worker after the API returns, not inside the production function.

Byte parsers/dependencies are unchanged. No fresh sanitizer, SDK, container, deployment,
v2 live service or production-readiness claim is implied.

## Subsequent operator path

The [retained-file path API and CLI](account-file-restore-cli.md) now supply the
previously separate operator entry point. Source confirmation occurs inside the
last common locked proof; the byte API keeps its existing immutable-input contract.
Explicit live v2 startup/adoption remains open.
