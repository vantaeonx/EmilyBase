# ADR0155: joined immutable account/file archive bytes

Status: accepted for experimental bounded byte encoding/verification only.

## Decision

Use a distinct EMILYAFB version1 envelope around copied canonical v2 declaration,
existing v1 account bundle and strict project-ordered existing paired file archives.
Keep every nested format unchanged. Use a fixed128-byte header, complete body
SHA256/header CRC, exact lengths, zero reserved fields and48-byte file frames.
Check outer bounds/integrity before nested replay, and exact private/file roster,
file database bindings and globally distinct public/private/file IDs afterward.

Apply the collector's four-file/128-physical-object/128MiB encoded-image limits
to untrusted archive contents too. Charge all orphans. Bound complete encoded
input by128MiB+4672 bytes:128 outer header, four48+192+128 file/archive headers and
128 object frame headers of24 bytes beyond charged native images. This is framing
accounting, not whole-process memory admission. Nested models and scratch remain
additional costs under their existing finite bounds.

Offer opaque borrowed VerifiedAccountFileArchive and AccountFileArchiveReader
from successful native snapshots or verified immutable input. The reader owns
only small framing/native readers and borrows actual complete images. Hash through
8192-byte scratch and support checked Read/Seek without another full payload copy.
Never obtain source descriptors, run mutable storage, reset sessions or infer
current permission while encoding/decoding. Debug shows counts/lengths only.

## Consequences

Checksums detect changed bytes, not origin authentication. Parsed bytes are not
original-source ownership or a durable backup receipt. Existing v1 root startup
still refuses a v2 filesystem manifest; no root layout is installed. Native paired
archives can be recovered independently, but common no-replace publication and
original-owner restore require separate implementations and failure matrices.
Do not compose restore by closing components and adopting filenames afterward.

Validate independently constructed empty goldens, all header/raw truncation/body
mutations, repaired semantic/framing failures, exact native-byte equality, borrowed
range/seek models, actual128/129 valid-object pairs, arbitrary inputs and a new
pure fuzz target with accepted mixed synthetic seeds. Record sanitizers and
source-bound stable/minimum regressions separately from earlier full CI evidence.
