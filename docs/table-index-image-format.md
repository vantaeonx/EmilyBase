# Bound primary-tree images (EBTI version 1)

Experimental optional derived-cache images. The committed relational WAL remains
authoritative. The current library exports/verifies/loads bytes; it neither
publishes a sidecar nor automatically loads one during database recovery.
Existing database, page, catalog, WAL and backup bytes do not change.

All integers are unsigned little-endian. The outer header is exactly 128 bytes.

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 8 | `EBTI\0\0\0\0` |
| 8 | 2 | version 1; unknown versions rejected |
| 10 | 2 | header length 128 |
| 12 | 4 | reserved zero |
| 16 | 16 | persistent WAL database identity |
| 32 | 8 | acknowledged transaction number |
| 40 | 8 | nonzero table identity |
| 48 | 32 | SHA-256 of every exact encoded relational page, in iteration order |
| 80 | 8 | exact nested payload length |
| 88 | 32 | SHA-256 of nested payload |
| 120 | 4 | reserved zero |
| 124 | 4 | CRC32 of outer bytes 0..124 |
| 128 | variable | complete version-1 stable-ID EBIF snapshot |

The nested revision must equal the outer acknowledged transaction number.
Complete size is bounded to 8320..4198528 bytes before parsing. Payload size must
match exactly; trailing bytes, unknown/reserved fields, bad checksums, malformed
pages and incomplete topology are rejected. Nested root/counts, sorted unique
page IDs, occupancy, key order, separators, reachability and leaf links use the
existing [index snapshot rules](index-format.md).

`inspect_primary_index_image` verifies structure only and returns transaction,
table identity, entries, page count and root. Those fields are not proof of scope,
authorization or current data. Checksums detect damage; they are not signatures.
The image contains primary-key values and must be treated as private application
data. `*.table-index` is excluded from Git.

`Database::primary_index_image` exports a stable-ID copy after checking every
eligible live key and actual pointer. `verify_primary_index_image` and
`load_primary_index_image` require the exact persistent database/table IDs,
acknowledged transaction and page-history fingerprint. They then compare the
complete eligible key set and every page/slot to current live row locations and
resolve each actual relational record. A tree with missing/extra keys or obsolete
pointers cannot load, even if all envelope hashes were repaired.

All integer and <=256-byte text primary keys belong to the tree. Longer valid
text keys remain in the table and use the existing map path. Empty/long-key-only
tables still have one empty root. Installing changes only the target derived
cache cell. It writes no WAL, relational page or file; failed validation preserves
the previous cell and logical data. Later writes stage normal stable-tree
insert/delete/pointer maintenance. Historical snapshots retain their own cells.

Binding covers the whole relational history: a committed change to a sibling
table invalidates the image as well. Rollback and empty commits do not. Current
checkpoint, both WAL versions, baseline compaction and verified restore preserve
page images, transaction number and database identity, so matching images remain
valid. Restore intentionally creates an independently writable clone with the
same database identity; mutation of that clone retires only its current binding.
Future history vacuuming may invalidate these optional images.

The pure Snapshot export/verify/install APIs validate the row projection but have
no persistent identity binding. Managed callers use the Database wrapper. No
filesystem cache adoption, independent index commit/root record, secondary-index
DDL or recovery performance claim is included. See [ADR 0023](adr/0023-bound-primary-tree-images.md).
