# Experimental object archive v1

The original synchronous object archive codec encodes immutable verified
[captures](object-inventory.md) and fully verifies borrowed archive bytes.
Encoding returns a byte vector; it does not publish a durable backup or restore a
directory. Current AccountRoot backup formats remain unchanged and exclude objects.

All integers are little-endian. Maximum count is128, combined payload64 MiB,
individual payload8 MiB, and complete archive67,124,352 bytes. Unknown versions,
flags/reserved fields, duplicates, unsorted IDs, truncation, trailing bytes and
scope/length/checksum mismatches refuse before returning a verified view.

| Offset | Bytes | Value |
| --- | ---: | --- |
| 0 | 8 | `EMILYOBK` |
| 8 | 2 | Version1 |
| 10 | 2 | Flags, zero |
| 12 | 4 | Header bytes,128 |
| 16 | 16 | Expected project ID |
| 32 | 4 | Object count, at most128 |
| 36 | 4 | Reserved, zero |
| 40 | 8 | Combined payload bytes, at most64 MiB |
| 48 | 8 | Exact framed body byte count |
| 56 | 32 | Canonical inventory SHA-256 digest |
| 88 | 32 | SHA-256 of complete framed body |
| 120 | 4 | Reserved, zero |
| 124 | 4 | IEEE CRC32 of preceding124 header bytes |
| 128 | Declared body length | Complete ordered object frames |

Each frame is16 raw object-ID bytes, an8-byte envelope length, then the exact
EMILYOBJ image. IDs must increase strictly by raw-byte order. The nested envelope
must match the trusted expected project and outer frame ID, including full payload
SHA-256/header CRC checks. Every envelope is96 through8 MiB+96 bytes. Body length
equals payload bytes plus120 times the object count. Empty archives have no frames
and retain the canonical scoped empty inventory digest.

The parser bounds the input/count before allocation, verifies outer body integrity,
checks every frame/nested image, totals exact payload bytes and recomputes the
original [inventory digest framing](object-inventory.md). It returns borrowed
verified objects only after the entire archive passes. Private constructors and
immutable borrows preserve that checked view; Debug hides payload. Canonical
re-encoding of a verified view preserves every byte.

Checksums detect corruption, not a hostile writer manufacturing a new valid image.
Neither archive fields nor a verified view grant user/project access, authenticate
a signer or encrypt contents. Expected project comes from trusted caller context.
Version1 remains experimental, without stable compatibility promises. Incompatible
changes need a new version and explicit separate-output conversion tests.

Native `inspect_archive_file` reuses bounded private readonly file admission,
final no-follow/nonblocking open, before/after change checks and visible inode
validation. Accept only regular singly linked0600/0400 files; reject oversize before
allocation. Paths/ancestors are operator authority and administrators remain
trusted. Inspection is a checked snapshot, not a retained filesystem lease.

`emilybase object-archive-verify PATH PROJECT` validates project shape before
opening input. Success prints format/project/object-count/bytes/digest JSON only.
Invalid archives produce no partial stdout. Output failure leaves source bytes
unchanged. No archive-write or restore CLI is implemented in this block.

Independent Python struct/hashlib/zlib bytes, every prefix/byte corruption,
resealed invalid headers/body, nested scope, strict order/uniqueness, exact maximum
archive/capture/native inspection, generated binary models and actual CLI tests
cover the codec. Fuzz invokes the real decoder on raw inputs and on bounded copies
with only outer checksums recomputed, reaching structural checks as well. It does
not prove filesystem consistency, backup publication or restore correctness.
See [ADR0120](adr/0120-checked-object-archive-format.md).

Durable archive publication, separately verified restore, root integration,
streaming/compression/encryption, HTTP/file policies, signed URLs and production
acceptance remain open. Use synthetic data only.
