# Native paired archive file restore and CLI

The experimental paired archive can now be verified and restored by the native
operator CLI. This uses the original Rust engine and native object storage. The
commands do not provision users, authenticate an archive's origin or grant file
access to a session.

```sh
emilybase file-archive-verify copy.file-archive 01010101010101010101010101010101
emilybase file-archive-restore copy.file-archive 01010101010101010101010101010101 restored
```

The expected project is explicit and canonical. Native paths are chosen by the
trusted operator. Verification is readonly. Restoration requires a fresh common
destination; it never overwrites a database, rescopes projects or repairs input.
Existing parent directories must exist. The archive must be a regular private
0400/0600 single-link file. Leaf and immediate parent symlinks refuse; ancestors
remain a native trust boundary. FIFO opening is nonblocking and its type refuses.
The existing encoded-size limit is checked before allocating the image.

## Source ownership through selection

`restore_file_archive_file(path, expected_project, fresh_destination)` retains the
original input file and parent descriptors. Its one bounded immutable image is
verified before any stage is created. Original inode, private mode, link count,
size, timestamps, visible file/parent selection and exact bytes are rechecked after
reading, before staging, before common-root selection, and after selection. Full
byte comparisons use8KiB scratch without another complete image allocation.

The [common restore](native-file-archive-restore.md) continues retaining both actual
child directories, restored engine/object owners, original WAL and all physical
objects through its original no-replace/fsync/readback rules. A preselection input
failure leaves the destination absent and preserves any nonempty private common
stage. A late input change is OutcomeUnknown; the selected root remains available
for explicit inspection. No source file is modified or repaired.

The final checks are observations, not a lease against native administrators or an
atomic snapshot of all namespaces. The image and metadata replay have finite bounds,
not a global memory or descriptor reservation. Untrusted HTTP paths, AccountRoot
coordination, current file policies, signed URLs and production acceptance remain
separate work. See [ADR0142](adr/0142-retained-native-file-archive-input.md).

## Result delivery

Successful commands print one count-only JSON record: format, expected project,
metadata identity/WAL version/history/counts, persisted physical quota, reference
count and physical object count/bytes/digest. Transaction numbers are decimal
strings. Names, owner fields, payloads and credentials are omitted.

Process success requires the final stdout write and flush. If stdout fails after
durable restoration, the process fails but the selected database remains. Failure
to receive the report is not permission to overwrite or blindly retry: inspect the
existing root. The source archive can still be independently verified. This CLI
does not yet create a new live file catalog or expose an HTTP file service.

## Executed evidence

[Source-bound verification](measurements/2026-10-10-file-archive-cli/verification.json)
records both supported toolchains. Native readonly400 restoration preserves WAL1/2,
empty/8193-byte payloads, one charged orphan, exact canonical paired bytes and later
independent writes. The actual input descriptor is measured while all restored
owners are admitted; the descriptor count returns exactly after completion.

Forty-eight input changes span both WAL versions and after-read, preselection and
postselection boundaries: identical-byte new inode, same-inode corruption, public
mode, hardlink, moved/replaced parent, same-byte rewrite, growth and truncation.
Preselection errors leave the target absent; postselection uncertainty preserves
the original independently reopenable pair. Invalid type/mode/scope/format/version,
symlinks, hardlinks, FIFO and an oversized private sparse input create no stage.

Sixteen new actual SIGKILL cases span read, original owners admitted, selected and
caller-success, both WAL versions and empty/8193-byte payloads. Original source
bytes remain unchanged; selected/acknowledged roots reopen exactly and accept the
next transaction. Actual CLI processes verify exact count-only output, relative
paths, binary/empty payloads, orphan quota, malformed/foreign inputs, FIFO deadlines,
no-replace refusal and failed stdout delivery. Preceding native crash cases rerun.

The paired decoder/header/encoder and pure fuzz target remain unchanged; no new
filesystem sanitizer campaign or physical power-loss evidence is claimed.

Final affected matrix:606 checks on each Rust1.99/1.89 (284 native files/storage/CLI
and322 database/WAL/transactions/backup), with611 source hashes unchanged afterward.
Workspace/fuzz formatting, strict all-target lint, affected builds and minimum fuzz
builds pass. Sixteen new and115 preceding native process kills pass per toolchain.
Unchanged shared component APIs preserve the preceding separate account/Root/registry
regression evidence; that earlier evidence is not a new full-workspace run.
