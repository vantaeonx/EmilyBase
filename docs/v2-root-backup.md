# Complete backup of an explicitly opened v2 root

Experimental trusted native maintenance, using synthetic data. This does not grant
current file/user authority or make the platform ready for production.

```sh
emilybase account-file-backup-v2 ./copies/root ./backups/second.archive
emilybase account-file-verify ./backups/second.archive
emilybase account-file-restore ./backups/second.archive ./copies/second --time 100
```

The backup parent must already exist privately0700, outside the original source.
Existing output names are never replaced. The explicit command opens only v2;
account-file-backup and the v1 capture APIs still reject v2 before staging. There
are no file-subset arguments, directory discovery or implicit format adoption.

## Full metadata and actual owners

AccountRoot::capture_v2 returns a complete immutable AccountFileSnapshot.
backup_account_file_root_v2(root,path) publishes it inside the same checked capture.
Both use the NativeFileRoot objects already held inside AccountRoot, without closing,
reopening, exporting mutable file handles or substituting detached copied receipts.

Retain the full original RootLayout::V2 and exact canonical manifest bytes. Match
every sorted file project/database binding to its actual inspected component. The
account bundle remains an unchanged v1 subcomponent of the existing joined archive;
AccountComponentSnapshot describes only those account bytes and their report. It
does not carry a synthetic v1 root manifest or erase enclosing file bindings. The
experimental AccountFileSnapshot::accounts accessor now returns this narrower type;
bundle_bytes/report remain available, while whole-root manifest access belongs to
the enclosing AccountFileSnapshot. No persisted format version changes.

Split private borrows into file owners, their original directory topology and the
account capture frame. Nested native file snapshots enclose account/public capture
and publication. Every actual payload reader, including orphans, spans publication
and its respective native final proof. Public writers span account capture; they
unwind before joined_backup_confirming. Native/private/common owners remain held
by the caller's AccountRoot. Payload readers have unwound by confirmation. The
selected archive descriptor remains held through every enclosing source proof and
the final selected-file check. These are finite checks, not a namespace lease.

## Physical source changes and bounds

A regression reproduced an incorrect successful receipt after an external private
WAL tail append at joined_archive_selected; the expanded case reproduced the same
fault for a public data WAL. Comparing only committed bundle digests missed the
unacknowledged physical suffix. Joined v1/v2 readonly capture now
retains independent original common-tree readers, hashes and identity/size/mode/
link/time stamps, then checks them after account/public capture unwinds. This reuses
the bounded physical-tree verifier used for joined restore; it does not trim or
repair the changed journal. Plain v1 AccountRoot::capture keeps its original
committed-prefix contract, excluding concurrent unacknowledged suffixes while
preserving them in sources. Joined capture/publication uses the stronger physical
proof. Preexisting tails remain excluded from copied committed data and unmodified.

The common physical-tree bound is512 directories,1024 files, depth6 and384MiB of
physical input, with8192-byte hash scratch. It includes public/private caches and
v2 file components in addition to their native proofs. These are physical/I/O bounds,
not a decoded-model or total heap reservation. File admission remains four stores,
128 aggregate physical objects including orphans, and128MiB retained encoded images.
Admit object counts before retaining capture readers; charge copied images after
each bounded component copy. Decoded models and temporary buffers are additional.

Wrong format, scope/order/identity, excessive budgets, busy owners, contained output
or an existing destination refuse. Observed source/selection failure after archive
publication returns ComponentPublicationUnknown, withholds the receipt and preserves
the selected archive and external bytes. Do not treat this as rollback or remove
unknown data. Inspect the preserved artifact and choose a fresh output name to retry.
Late output failure also preserves the complete archive and releases source owners.

The nine CLI output fields match account-file-verify: format, projects,
private_projects, file_stores, references, physical_objects, copied_bytes,
archive_bytes and decimal-string reset_at. IDs, names, rows, payloads, password
verifiers and sessions are excluded. Archive bytes themselves remain sensitive and
unencrypted; private permissions do not provide encryption.

## Verification boundaries

[ADR0165](adr/0165-complete-v2-root-backup.md) records the decision and
[source-bound results](measurements/2026-10-19-v2-root-backup/verification.json)
record executed commands, failures reproduced before correction, final hashes and
test counts. Dedicated tests cover empty/one/four stores, both WALs, exact bindings,
current session survival, independent restored writes, physical source changes,
selected archive replacement, contained outputs, aggregate admission, parent moves,
sync errors, generated models, actual signal9 workers and actual CLI roundtrips.

Ordinary server-binary v2 configuration/restart/container/upgrade drills, current
user file permissions, roles, aggregate network admission, realtime, the dashboard,
Kotlin, encrypted backups and production acceptance remain separate open gates.
