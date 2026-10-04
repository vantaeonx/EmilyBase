# Experimental file format v1

All integers are little-endian. Pages are exactly 4096 bytes. The file contains
one header page followed by zero or more data pages; its length must be aligned.
There is no persisted page count. Initially files are bounded to 65536 data pages.

## File header (physical page 0)

| Offset | Bytes | Value |
| --- | --- | --- |
| 0 | 8 | `EMILYDB\0` |
| 8 | 2 | format version: 1 |
| 10 | 2 | page size: 4096 |
| 12 | 4080 | reserved, all zero |
| 4092 | 4 | CRC32 of bytes 0..4092 |

## Slotted data page

| Offset | Bytes | Value |
| --- | --- | --- |
| 0 | 4 | `EBPG` |
| 4 | 2 | page format version: 1 |
| 6 | 2 | page kind: 1 (records) |
| 8 | 8 | physical page ID, at least 1 |
| 16 | 2 | slot count |
| 18 | 2 | end of slot directory: 32 + 6 × count |
| 20 | 2 | beginning of packed payload |
| 22 | 6 | reserved, zero |
| 28 | 4 | CRC32 of bytes 0..28 followed by 32..4096 |
| 32 | 6 × count | slot directory |

Each slot contains a u16 offset, u16 length and u16 state (0 deleted, 1 live).
Deleted slots have zero offset and length. Live records are packed backwards
from the end of the page without overlap or gaps; empty records are allowed.
The free region between slots and payload is zero. A record is at most 4058 bytes.
Deleted slots may be reused. Updating or deleting compacts the payload without
changing other slot numbers. A reused slot is not the same logical record.

CRC32 detects random corruption, not deliberate tampering. Invalid sizes,
versions, reserved bytes, IDs, checksums and slot layouts are rejected.

## Compatibility

Version 1 is experimental, not a promised stable format. Incompatible changes
must increment the version, update this document and have explicit conversion
tests. Unknown versions and page kinds fail closed; never open them for writing.
Any migration must keep the original file and use a separately verified output.
Golden fixtures are synthetic and represented as source bytes, not user files.

## Durability boundary

Creation pins the destination directory and exclusively creates a random 0600
temporary file through that handle. It syncs the complete initial image, rereads
exact header/page bytes, validates identities and single-link admission, then
publishes with Linux no-replace rename and syncs the owned directory. A returned
pager retains the same inode and exclusive lock. Final parent symlinks are refused;
operator-selected ancestors remain trusted. The explicit directory-handle API
accepts one filename and addresses the owned directory even if its name moves.

Before publication, cleanup removes only the unchanged owned staging entry.
After rename, sync or identity/admission failure preserves the selection and
reports `PublicationUnknown`; reopening/inspection is required before retry.
Raw open refuses final symlinks, multiple hard links and nonregular files. These
are filesystem admission rules; header and page bytes remain version 1.
See [ADR 0034](adr/0034-owned-page-file-publication.md).

In-place page writes are synced but are **not transaction-safe**. A crash can
tear a page or leave an incomplete append. The initial pager detects corruption;
it cannot repair it. The separate managed transaction layer uses its mandatory
WAL for recovery and rollback; this raw pager does not implement those guarantees.
Do not equate an individual page-write result with a durable transaction commit.
