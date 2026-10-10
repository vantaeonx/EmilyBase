# Bounded immutable-reader file publication

`publish_private_reader_at_retained(directory, name, source, exact_bytes, maximum)`
publishes one fresh private native file from a synchronous Read+Seek source. The
trusted caller owns the destination directory and keeps source bytes immutable
through both passes. The reader must honor standard contracts. This is neither
user authority nor admission for an untrusted network stream.

Declared bytes above the caller's maximum refuse before source or filesystem I/O.
The name must be one component in the actual retained directory. The original
Pending machinery creates its random exclusive0600 stage. Reset the source to
zero, copy exactly the declared bytes with8192-byte scratch, then probe one byte
for exact EOF. Short input returns SourceLength; trailing input is consumed by at
most one extra byte and is never written. Other reader/seek errors remain typed I/O
failures. Interrupted reads retry; this does not establish a deadline.

After the original file fsync, check owned staging identity/private metadata and
exact length. Rewind the source and stage, compare every byte using two8192-byte
buffers, and require source EOF again. Length/time metadata must remain stable
across this readback, and owned staging admission is rechecked. A byte mismatch
returns Readback; changed native metadata/identity refuses selection. A mutable
source that changes both itself and native storage consistently is outside the
immutable-source/native-owner contract; a report is not a later filesystem lease.

Clone the selected descriptor before rename, so descriptor exhaustion is still a
preselection failure. The existing no-replace rename, parent fsync and selected
identity/private-file checks are unchanged. Preselection cleanup only removes the
still-owned random stage. Unknown/substituted entries are preserved. After rename,
sync or selected identity failure returns PublicationUnknown and preserves the
selected result for explicit inspection. Blind replacement/retry is forbidden.

The existing byte-slice publishers keep their original implementation and checks.
No storage page/WAL format, transaction commit acknowledgement or parser changes.

## Native archive use

Object backup now passes the [immutable ArchiveReader](borrowed-object-archive-encoding.md)
over its complete checked capture to this publisher. It still pins the destination
parent before capture, compares the complete current source inventory before
staging, retains the selected archive descriptor, and performs complete archive/
nested-object/digest/private-metadata/visible-inode/parent/source-marker readback
before returning metadata. Captured bytes and original format remain unchanged.

The complete source snapshot still owns up to64 MiB of payload plus envelopes.
Encoding now retains bounded frames instead of a second complete output image;
storage copy/readback and selected archive inspection use bounded scratch. This
does not stream live capture, change standalone restore memory, enforce persisted
quotas, integrate AccountRoot files or qualify production use. See
[ADR0131](adr/0131-bounded-reader-publication-and-object-backup.md).

## Executed checks and reproduced fixture interference

Stable1.99 and minimum1.89 each passed199 relevant checks:118 object,54 storage
and27 CLI. Eleven new regular cases include64 generated binary publications,
exact empty/short/chunked/Interrupted sources, ten read and two seek fault points,
five short/trailing/between-pass changes, four original synchronization failures,
six staged content/length/mode/link shapes, a late readback mutation, foreign stage
substitutions and uncertain selected substitutions. Actual maximum128-object/
64 MiB CLI backup restores into a fresh independent directory; both inventories
equal the original. Five new actual kills cover copy, synced stage, selected name
and received full/empty success. Existing20 kill boundaries reran as well.

The first stable matrix failed a pre-existing immediate pager reopen with Busy;
eight other failures followed from its poisoned test mutex. A bounded reproduction
failed again on attempt3. The new subprocess fixture was not participating in the
existing creation-test serialization. It now acquires that same test mutex before
spawning, preserving all immediate lock/reopen assertions and runtime lock code.
Twenty subsequent complete native storage-library repetitions passed43 cases each.
Both final supported-toolchain matrices then passed on580 unchanged frozen hashes.
Failed attempts are excluded from the green results.

The observed interference is consistent with an inherited open file description
temporarily retaining a lock until exec closes descriptors; the exact syscall
interleaving was not traced. Linux documents duplicated/forked descriptor lock
lifetime in [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html). The test
serialization fix is also used by the existing creation subprocess fixture.

Formatting, strict stable workspace/fuzz lint, builds and minimum all-fuzz
compilation exit0. No parser or fuzz target changes in this increment; the previous
761600-input ASAN comparison belongs to33e3242 and is not a filesystem fuzz result
for this publisher. The earlier full1518-test baseline belongs to d988a51.

Five fresh actual CLI backup processes per source/shape preserve complete source
hashes, archive bytes/hashes and metadata. For an eight-object64 MiB payload,
median peak RSS was138304 KiB on33e3242 versus73396 KiB here. For one64 KiB object,
8236 versus8716 KiB shows no small-shape improvement. GNU time measures total
process peak RSS for local Rust1.99 development binaries with debug0, not requested
heap, cold memory, speed or a whole-process quota. The existing owned capture
explains why this does not become constant-memory backup. See the
[source-bound report and raw samples](measurements/2026-10-10-reader-publication/verification.json).
