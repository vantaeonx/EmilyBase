# Coordinated joined backup and operator cycle

`backup_account_file_components(accounts, files, destination)` publishes a private
joined archive from the caller's original v1 account root and explicit native file
roots. Publication runs inside retained component capture. No source is closed and
reopened as a substitute for its original owner. Session clocks/scopes, credentials,
admission and source bytes are not intentionally changed.

The operator can now create, verify and restore a joined archive:

```sh
emilybase account-file-backup ./account-root ./backups/snapshot.bin
emilybase account-file-verify ./backups/snapshot.bin
emilybase account-file-restore ./backups/snapshot.bin ./copies/restored --time "$(date +%s)"
```

The first command deliberately selects no file stores. To include files, supply an
existing private0700 parent of project-named native roots and explicitly repeat
`--file-project` for each required canonical project ID (at most four):

```sh
emilybase account-file-backup ./account-root ./backups/with-files.bin \
  --file-parent ./native-files --file-project "$PROJECT_ID"
```

PROJECT_ID is existing native project metadata, not a service key. Duplicate,
malformed, excessive or parent/roster-inconsistent arguments refuse. Only the
explicit list is sorted; arbitrary filesystem entries never become a file roster.
Every selected project must already belong to the account root's declared private
roster. Open original existing file roots relative to that one retained parent;
never create missing account/file stores or infer permission from an owner field.
The parent descriptor remains native authority across moves; no namespace lock or
current account permission is issued.

Both account-file-backup and account-file-verify emit the same bounded nine-field
count report: format, projects, private_projects, file_stores, references,
physical_objects, copied_bytes, archive_bytes and decimal-string reset_at. They
print no rows, names, project IDs, password verifiers, tokens or payloads. The
archive itself contains sensitive copied data: keep it private, outside Git.

## Original directories and lifetime boundaries

Before staging, refuse a destination parent equal to or inside any actual account
or selected file root. Storage OutsideDirectoryGuard independently opens each
original source/destination directory; it never duplicates a source's locked open
description. Compare dev/inode while walking original destination ancestry, at most
256 directory observations. Excessive ancestry refuses. Repeat comparisons around
streaming/seek, selection, readback, acknowledgement and final acceptance. No caller
receives a source descriptor, writable owner or filesystem lease from this guard.

The target parent/path is selected once through the existing private no-replace
publisher. Its original pathname must keep selecting that parent; moving/replacing
it refuses, and foreign former paths are not adopted. A source selected through
the native open_at APIs can move with its original retained parent. These are finite
native filesystem observations, not protection against privileged namespace/mount
changes or a promise that concurrent renames cannot occur between observations.

Original public database writers, private owners, native file owners and every
payload reader span the innermost archive publication and their respective final
source proofs. Payload readers unwind with native file capture; public database
writers unwind with registry capture. The account root/private/native file owners
remain caller-held. The actual selected archive file and baseline remain retained
through capture unwind and the final destination/ancestry/content-stamp check.
Do not claim every component writer/payload reader remains locked until return.

Successful publication followed by a source failure returns
ComponentPublicationUnknown, discards its receipt and preserves the archive. A
publisher uncertainty cannot be hidden by a later source failure. Final archive
replacement, byte/mode/link/stamp change or parent change also withholds success.
Existing destinations are never replaced. An output-consumer failure can follow
durable completion: the CLI exits unsuccessfully but leaves the completed archive.
Inspect before retrying; no rollback or unowned cleanup is promised.

## Verification and remaining gates

[ADR0163](adr/0163-coordinated-joined-backup-cli.md) records the composition.
[Source-bound results](measurements/2026-10-18-operator-backup/verification.json)
record the final commands, source hashes and exclusions. New source cases cover
empty/one/four-project inputs, both WALs, exact archive bytes, original sessions,
independent restored writes, actual descriptor/lock lifetimes, contained targets,
12 late source mutations, six post-capture archive changes, four sync failures,
source-parent moves, 16 generated explicit-subset models and 12 actual signal9
kills including a caller-received checkpoint after API return. Storage tests prove
ancestry bounds, original-inode comparison after moves and no inherited flock.
Five actual binary cases cover the full cycle, private count receipts, explicit
subset/argument/membership admission, contained output, conflicts, busy owners,
missing stores, parent aliases/modes and failed stdout without source changes.

This is trusted native offline maintenance. Source formats and locked dependency
versions are unchanged; CLI now directly enables the already-used rustix fs API.
The restored v2 root remains offline: ordinary v1 startup refuses it, and this v1
backup command cannot silently adopt/rebackup v2 or drop its file bindings. Explicit
live v2 adoption, aggregate process admission, current user file policies, encrypted
backups, deployment/upgrade drills and production acceptance remain separate work.
