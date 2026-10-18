# Joined restore from a retained private file and CLI

`restore_account_file_archive(archive, destination, pool, now)` retains the actual
source archive file/parent through preparation and the last common locked proof.
The CLI exposes this offline operation:

```sh
emilybase account-file-verify ./snapshot.bin
emilybase account-file-restore ./snapshot.bin ./restored --time "$(date +%s)"
```

The input is the existing joined archive produced by the native capture/publisher
API. Verification alone emits inventory and creates no destination. Restore requires
one fresh destination under an existing private0700 native parent. It never replaces
an existing file/directory/link. `--time` is mandatory trusted operator Unix time,
limited to the existing signed timestamp domain; it is never a client clock.

Successful stdout is one bounded JSON line with root_format, projects,
private_projects, file_stores, references, physical_objects and reset_at. Time is a
decimal string, preserving values beyond JavaScript's exact integer range. Output
contains no rows, project/file display names, password verifiers, tokens or payloads.
The source archive is sensitive: retain it privately and never commit it to Git.

## Source and destination authority

Open the source without following a link or blocking on a special file. Require a
regular single-link0400/0600 file including special permission bits, with length at
most MAX_ACCOUNT_FILE_ARCHIVE_BYTES. Retain the original source parent/inode and a
bounded immutable encoded image. Check exact EOF, original selected filename,
identity/size/mode/link/time stamps and complete bytes through8192-byte scratch.
No source descriptor or mutable storage escapes to the caller.

Purely verify that immutable image before staging. Source confirmation runs inside
the common restore protocol before staging/selection and around the final physical
proof while the common root lock and original physical readers are still held.
Checking the source only after the byte restore API returns would release the
common lock too early; this path API deliberately avoids that composition.

The captured destination parent is native authority across a move. A replacement
at its old pathname is never adopted. The source path, in contrast, must continue
to select its original parent/file: moving/replacing that source parent withholds
confirmation. Non-UTF8 native source leaves are supported; destination leaves must
fit the existing UTF8 native root API. Source/destination parent links are refused.
Checks are finite observations under trusted native OS ownership, not namespace
leases, authenticated backup provenance or encryption.

Before common selection, source change refuses publication and may preserve a
nonempty private stage. After selection, observed source change is
ComponentRestoreUnknown: discard the receipt and preserve the selected copy for
inspection. Do not repair changed input, retry blindly, remove selected data or
promise rollback of acknowledged preparation. Source/destination overlap is an
existing-target conflict, never an overwrite.

A stdout consumer failure can happen after a complete durable copy has been
created. The command exits unsuccessfully and preserves that copy. Inspect it
before retrying; a lost printed receipt does not mean the restore rolled back.
Argument/type/privacy/size errors produce no metadata line and do not publish a
root. Invalid timestamps are rejected before opening the source.

## Runtime boundary and verification

This CLI prepares an offline v2 root. Ordinary v1 AccountRoot/server startup still
refuses v2; explicit live v2 adoption and current user-file authorization remain
open. Original data/sessions stay unchanged; copied sessions get a fresh scope/time
and v5 admission closes, following the [common protocol](account-file-root-restore.md).
Native operators can independently inspect/use completed components after return.

[ADR0162](adr/0162-retained-joined-input-and-restore-cli.md) records the decision.
[Source-bound evidence](measurements/2026-10-17-joined-restore-cli/verification.json)
records actual final checks and exclusions. Seven new source cases cover readonly/
writable inputs, immutable archive/source sessions, descriptor release, independent
copied writes, parent moves, privacy/type/size/overlap and16 generated models.

Sixty source mutations span both WALs and input-read/input-confirmed/prepared/
selected/finalizing boundaries: same-byte replacement, tail, truncation, mode,
hard link and source-parent replacement. Tests observe the source reader/common
lock during the last proof after component writers unwind. Ten actual signal9
kills distinguish input-read without staging, prepared, selected, finalizing and
actual caller receipt; fresh retries preserve sources and work independently.

Seven actual binary cases cover empty/nonempty copies and file WAL1/2, exact redacted
JSON, links/modes/directory/FIFO/oversize/corrupt input, required/invalid time and
invalid destination, existing targets, source overlap, failed output, native parent
links and non-UTF8 source names. The maximum signed timestamp is emitted/reopened
exactly. A ten-second child deadline makes a blocking FIFO regression observable.

Formats, byte parsers and dependencies are unchanged. This adds no automatic v2
startup, source authentication, sanitizer campaign or production-readiness claim.
