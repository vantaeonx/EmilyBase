# Experimental backup format, version 1

A backup is one bounded binary file. It contains a fixed header and the exact
acknowledged WAL prefix. Checkpoint files and uncommitted tails are excluded.
There are no archive entry names, filesystem paths, links or compressed payloads.
No existing database engine participates in encoding or restore.

All integers are little-endian. The 128-byte header has:

| Offset | Field |
| --- | --- |
| 0..8 | `EMILYBAK` magic |
| 8..10 | archive version u16, currently 1 |
| 10..12 | header size u16, currently 128 |
| 12..14 | embedded WAL version u16, 1 or 2 |
| 14..16 | page-file version u16, currently 1 |
| 16..32 | nonzero original database identity |
| 32..40 | last committed transaction ID u64 |
| 40..48 | exact WAL payload length u64 |
| 48..80 | SHA-256 of the payload |
| 80..124 | reserved zero bytes |
| 124..128 | CRC32 of header bytes 0..124 |

The payload cannot exceed the 64 MiB WAL limit; the entire file is at most
64 MiB + 128 bytes. Unknown versions, nonzero reserved fields, missing/trailing
bytes, checksum failures, identity mismatches, wrong commit boundaries and
invalid table history are rejected. Verification hashes the payload and executes
the same recovery and schema validation as a managed database open.

Creation owns the source journal exclusively. It compares recovered exported
pages with the current committed snapshot, writes an owner-only temporary file,
syncs it, rereads and verifies its exact contents through the owned file handle,
then publishes by descriptor-relative Linux no-clobber rename and syncs the
owned parent directory. Device/inode checks bind the visible destination and
staged entry to their handles. Source bytes do not change.
Interrupted temporary files are private and excluded from Git.

Restore verifies the complete archive before creating a staging directory.
Inside that owner-only directory it writes/syncs the original WAL, opens it with
the expected identity, compares the restored WAL byte-for-byte with the archive,
and materializes a checkpoint. Linux `renameat2` with `RENAME_NOREPLACE` publishes
the complete directory without replacing even an existing empty directory.
Restore operations use the staged directory handle through `/proc/self/fd`.
Syncing its owned parent and checking the selected entry precede success.
No partly initialized final directory is published. Errors before publication
clean up only staging entries still bound to owned handles; process kills
may leave private staging artifacts. An error after publication reports uncertain
durability or changed destination identity; verify the destination before retrying.
Substituted entries and detached original staging objects are preserved.

Restores preserve database identity and transaction IDs: they are historical
clones, not newly isolated projects. New commits remain possible after restore.
Legacy direct-write page files cannot be backed up by this managed-WAL API.
Both retained version-1 and compacted version-2 WALs are supported. The envelope
must match the actual payload version. An incomplete baseline is rejected even
if archive length and SHA-256 were recomputed. Existing version-1 archives remain
readable; old readers reject archives binding the new WAL version.

SHA-256 detects corruption; it is not authentication or encryption. Backups
contain plaintext data and require private storage. Source/target paths are
trusted local operator inputs. Final archive/parent symlinks and nonregular
archive inputs are refused without following or blocking on them. Ancestors
remain operator-trusted; this API is not a sandbox against malicious local
administrators. Linux and mounted `/proc` are required for publication/restore.
See [ADR 0032](adr/0032-owned-backup-publication.md). The format is experimental,
with no silent version conversion. Streaming,
encryption, incremental backup, remote storage and migration are not implemented.

## Application preparation before publication

The wire format stays EMILYBAK version1. restore_prepared permits a trusted local
application callback in the descriptor-owned private staging directory before
final replay/checkpoint/sync/no-replace publication. It must release engine owners.
Reports describe the prepared installed state; the immutable source may describe
an earlier commit. Application and backup errors remain distinct, and post-rename
uncertainty preserves installed state. Ordinary restore uses a no-op preparation.
The [private account wrapper](private-accounts.md) validates project/schema and
commits a fresh session scope before publication. This does not extend registry
archives or complete combined platform restore. See
[ADR0073](adr/0073-private-restore-reset-before-publication.md).

## Owned verified replay image

decode_verified validates EMILYBAK once and retains an immutable original-engine
RecoveredImage plus its report. VerifiedBackup has redacted Debug, private
construction and no filesystem owner or request authority. Its snapshot outlives
input bytes and source directory. Existing inspect_bytes drops it after returning
the same report. Per-format bounds do not reserve a numeric whole-process heap.
The [private inspector](private-accounts.md) applies complete account semantics
without replaying the same payload twice. See
[ADR0074](adr/0074-owned-verified-private-archive-inventory.md).
