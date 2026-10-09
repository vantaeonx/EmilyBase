# Bounded streaming object metadata verification

`verify_stream` checks one complete original v1 object envelope from a synchronous
reader and returns metadata only. It keeps a96-byte header and an8192-byte payload
scratch rather than constructing an owned payload image. This is a code-level
scratch bound, not measured whole-process memory admission or a zero-allocation
claim about callers, reader implementations, inventory metadata or filesystem caches.

## Exact contract

The caller supplies the expected project/object and exact total byte length.
Lengths below96 or above96+8 MiB refuse before the first read. The original shared
header decoder validates CRC, magic, version, flags/reserved fields, expected scope
and exact physical/payload length before body reads. Header metadata alone is not a
verified object and cannot be constructed as a public verified capability.

Complete payload SHA-256 is accumulated in chunks no larger than8192. Truncated
reads refuse; ordinary I/O errors remain typed I/O failures. Interrupted reads retry.
After a matching hash, one probe byte must report EOF. A valid payload with trailing
data is not a valid exact stream. At most the declared total plus one byte is read.
No unverified body bytes are returned or emitted.

Byte limits do not provide a read deadline: a supplied reader can block or repeatedly
interrupt. Native callers use checked regular files; a future network adapter must
provide its own framing, timeout, worker/resource admission and authorization.
The synchronous core does not perform socket reads on Tokio's reactor.

## Native use

Readonly `inspect_file` opens a private singly linked no-follow regular file, captures
metadata, streams its report, checks unchanged metadata and visible inode. Complete
project inventory does the same for every canonical name, retains those inodes,
rechecks all names/metadata and streams complete bytes again before its final scope
check and deterministic inventory digest. No per-object complete Vec is retained
while inspecting metadata. Existing count/total-byte limits remain enforced.

Owned get/capture and exact postpublication object readbacks keep their complete
immutable byte images. Archive inspection/encoding/restore and namespace-lifetime
contracts are unchanged. Checksums do not authenticate data; native operators and
ancestors remain trusted. There is no HTTP download/upload or user-file capability.

## Verification scope

Tests cover an independent Python struct/hashlib/zlib fixture, empty/scratch-boundary/
maximum8 MiB payloads, every prefix/byte mutation, repaired header fields and foreign
scopes, short chunks/interruptions, header/body/EOF I/O failure, exact read counts and
64 generated binary readers. Native hooks mutate private mode, links, length or
visible names after hashing and require refusal without cleanup.

The original object-format fuzz target compares whole-byte and reader decisions,
checks canonical re-encoding, repairs header CRC to reach semantic fields, and
constructs canonical envelopes from every input to reach complete hashing/EOF work.
Actual campaign results belong in source-bound evidence; existence of this target
is not a fuzz or production acceptance result. See
[ADR0126](adr/0126-streaming-object-metadata-verification.md).
