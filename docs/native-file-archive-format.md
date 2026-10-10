# Native paired file archive v1

Experimental standalone format; no production compatibility or authenticated
origin claim. It contains one original-engine metadata backup and one complete
native object archive. Existing component formats and private schema1 remain.
See [ADR0139](adr/0139-paired-native-file-archive-format.md).

## Encoding

All integers are unsigned little-endian. The192-byte header is followed directly
by metadata backup bytes and then object archive bytes, in that order.

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | Magic `EMILYFBK` |
| 8 | 2 | Format version1 |
| 10 | 2 | Header size192 |
| 12 | 4 | Flags, zero |
| 16 | 16 | Expected project identifier |
| 32 | 16 | Original metadata database identifier |
| 48 | 8 | Positive last acknowledged metadata transaction |
| 56 | 8 | Exact metadata backup byte length |
| 64 | 8 | Exact object archive byte length |
| 72 | 32 | SHA-256 of the complete metadata backup |
| 104 | 32 | SHA-256 of the complete object archive |
| 136 | 52 | Reserved, zero |
| 188 | 4 | CRC32 of bytes0..188 |

Each component has its existing minimum/maximum size. Total size is exactly192
plus the two lengths and at most `MAX_FILE_ARCHIVE_BYTES`:192 plus the existing
metadata and object archive maxima. Component bounds precede arithmetic/slicing.
No gap, overlap, alternative ordering, extra padding or trailing bytes is allowed.
Unknown version, nonzero flags/reserved bytes and noncanonical nested formats fail
closed. Future incompatible layout changes require a new archive version; this
experimental format does not make the underlying database format stable.

## Semantic verification

`verify_file_archive(bytes, expected_project)` verifies fixed bounds/header CRC and
expected project, then both complete outer SHA-256 digests before nested decoding.
The original metadata backup must fully replay, carry the same database identity
and last transaction, and satisfy exact private file schemas/scope/revision rules.
The object archive must satisfy its own project, ordering, hashes and size rules.
Persisted quota must fit all physical objects and payload bytes, including orphans.
Every logical reference resolves to one matching object hash/length; existing
metadata validation refuses duplicate object references. Missing/mismatched objects
fail even when both components are separately valid and outer hashes are correct.

VerifiedFileArchive owns bounded reference metadata and borrows immutable encoded
components/payloads. Generic backup replay is transient; it can allocate additional
bounded storage. Matching identities prove no unique ancestry across copied images,
current user authority or trusted backup origin. Checksums provide no signature.

## Borrowed encoder and scope

FileArchiveReader implements Read/Seek from [FileSnapshot](native-file-snapshot.md)
or VerifiedFileArchive. It hashes using8 KiB scratch, borrows existing immutable
component images and allocates no second complete encoded payload. Reads/seeks
create no filesystem owner. Invalid negative/overflow seek preserves its cursor;
seeking past EOF is permitted and reads return zero. Caller buffering, source
images, replay and cache remain outside any global memory budget.

This increment has no path publisher, common atomic restore, Root/user route or
signed capability. Canonical encoding/byte verification are prerequisites for
future owned no-replace publication and restore with their own crash acceptance.

## Executed evidence

[Source-bound checks](measurements/2026-10-10-file-archive/verification.json) include
both native WAL versions, exact header/partition checks, nested corruption with
outer digests repaired, independently valid incompatible component pairs, charged
orphans and canonical borrowed reencoding. Sixty-four generated read/seek histories
compare against flat independent slices; arbitrary bounded byte tests cover decoder
panic resistance. Previous source capture/mutation/quota crash checks are rerun;
no new archive publication crash result is implied.

The `file_archive` sanitizer target limits archive input to256 KiB after one mode
byte. Mode0 preserves raw bytes; mode1 repairs only outer SHA-256/CRC for nested
decoder reach. It never repairs sizes, identities or nested checksums. Sixteen
synthetic initial seeds cover WAL1/2, empty/reference/orphan/mutated-quota states
and both modes. Accepted images must satisfy independent graph accounting and
canonical byte-for-byte reencoding. A finite campaign is not a security audit or
testing every maximum-size128 MiB archive.

The final matrix passed585 checks per Rust1.99/1.89:264 native files/storage/CLI
and321 database/WAL/transactions/backup. All603 source hashes matched. Formatting,
strict workspace/fuzz lint, explicit builds and all minimum fuzz builds passed.
The83 preceding native process kills reran per toolchain. ASAN completed1483525
inputs in46s with maximum262145-byte input including selector and429/512 MiB RSS,
without findings. External dependency versions are unchanged. No complete current
workspace, maximum-size archive, publication/restore or security-audit claim.
