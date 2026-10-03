# Experimental B+ tree images, version 1

This standalone codec is separate from the current database-file layout. Dense
`page_images` exports pages ordered by ID; `from_pages` also requires a root ID.
Opt-in stable arenas and a canonical EBIF snapshot envelope now exist. Filesystem
publication, table root catalog and managed-WAL integration remain pending.

All integers use little-endian encoding. Every image is exactly 4096 bytes.

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 4 | `EBIX` magic |
| 4 | 2 | version 1 |
| 6 | 1 | kind: leaf 1, branch 2 |
| 7 | 1 | reserved zero |
| 8 | 8 | nonzero page ID |
| 16 | 2 | key count, at most 14 |
| 18 | 2 | payload end, inclusive lower bound 64, exclusive data end |
| 20 | 4 | reserved zero |
| 24 | 8 | leaf successor, zero for last leaf; branch requires zero |
| 32 | 8 | first branch child; leaf requires zero |
| 40 | 20 | reserved zero |
| 60 | 4 | CRC32 of bytes 0..60 and 64..4096 |
| 64 | variable | packed entries |

Each key begins with a one-byte tag and a two-byte length. Tag 1 requires exactly
eight signed-i64 bytes. Tag 2 carries 0..256 valid UTF-8 bytes. Keys are strictly
ascending and unique; integers precede text and text uses byte order without
collation or normalization. Empty text is a valid key.

A leaf key is followed by a 16-byte pointer: nonzero target page ID (8), slot ID
(2) and six zero reserved bytes. A branch key is followed by its right child ID
(8); the leftmost child is in the header. Child IDs are nonzero, unique within a
branch and cannot equal that branch's ID. Unused bytes after payload end are zero.
The 14-key cap fits maximum-size keys and leaf pointers in one page.

## Whole-tree rules

At most 1024 pages and 10000 entries; depth at most eight. Dense imports require
1..N IDs; stable imports allow holes within 1..1024. Except for the
root, every node has at least seven keys. A leaf root can be empty; a branch root
requires a key and at least two children. All pages must be reachable exactly
once. Leaves have equal depth, disjoint ordered key ranges and successor links
matching traversal order. Each separator equals the minimum key in its right
subtree. Missing/shared/cyclic children, orphan pages and wrong separators fail.

Insertion stages a bounded copy, splits overflowing leaves/internal pages and may
create a new root. A rejected operation changes no prior page image. Scans include
the start key, exclude the end key and return at most the requested bounded limit.
Page capacity may be reached before the entry cap for unfavorable insertion order.

`replace` changes only an existing key's row pointer, preserving the root and
index page IDs. Invalid keys/pointers or missing keys leave all images unchanged.
`remove` stages a bounded copy, rotates entries/child links from a sufficiently
populated sibling or merges adjacent nodes, recomputes exact separators, and
collapses a one-child root. It then validates the complete tree before publication.
Missing keys and failed operations preserve the original tree exactly.

Successful deletion remaps remaining arena IDs to dense 1..N, including the root,
child links and leaf successors. Opaque external row page/slot pointers never
change. Export all images with the current root; previously remembered index IDs
are not stable handles across deletion. This bounded in-memory arena is not a
durable page allocator or an incremental WAL write set.

That renumbering applies to dense mode. `new_stable`, `from_sorted_stable` and
`from_stable_pages` explicitly retain surviving IDs; allocation chooses the lowest
free ID. Retired IDs can later be reused. Sparse imports require increasing encoded
IDs and all whole-tree rules. Empty/root-collapse operations retain the selected
leaf/child ID. Dense APIs and frozen EBIX-1 bytes remain unchanged.

## Canonical EBIF snapshot envelope, version 1

`IndexSnapshot` contains a nonzero local revision and stable tree. Its byte codec
has one 4096-byte header followed by sparse EBIX-1 images in increasing ID order.
Exact size is `(page_count + 1) * 4096`, at most 4198400 bytes. Decode checks bounds
before allocating and verifies complete tree/count agreement.

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 8 | `EBIF` followed by four zero bytes |
| 8 | 2 | snapshot version 1 |
| 10 | 2 | reserved zero |
| 12 | 4 | page size 4096 |
| 16 | 8 | nonzero local revision |
| 24 | 8 | root page ID |
| 32 | 4 | page count, 1..1024 |
| 36 | 4 | reserved zero |
| 40 | 8 | entry count, 0..10000 |
| 48 | 12 | reserved zero |
| 60 | 4 | header CRC32 over 0..60 and 64..4096 |
| 64 | 4032 | reserved zero |

The revision is independent of table/WAL transaction IDs. The envelope does not
include project/table identity or authenticate row targets. No automatic converter,
file publisher or acknowledged snapshot durability is implemented yet.

`SnapshotDelta` is a validated in-memory write set without a wire/WAL codec yet.
It binds exact canonical base bytes using SHA-256 plus base/next revisions,
contains the final root/entry count, sorted changed/new images and sorted retired
IDs. Application rejects stale/different bases, repeated/reordered/overlapping
IDs, unchanged upserts, invalid topology/counts and revision overflow. It returns
a new snapshot after full validation; an empty delta can increment revision.
Retired handles need a future lifetime protocol. See [ADR 0016](adr/0016-stable-index-snapshots.md).

`from_sorted` accepts at most 10000 strictly ascending, unique, validated entries.
It balances leaves and then child groups bottom-up; non-root occupancy remains
at least seven keys even at group boundaries. A 10000-entry build uses 768 pages,
including with 256-byte text keys. It does not alter the borrowed source. These
operations keep the version-1 codec unchanged. Frozen digests obtained from the
published d75751b implementation cover empty, multi-level and Unicode images.

Version changes fail closed; there is no automatic converter. CRC detects
accidental damage and does not authenticate an index. Record targets, schema key
types, project ownership and pointer lifetime require future table-layer checks.
See [ADR 0010](adr/0010-index-maintenance.md) for costs and integration requirements.
