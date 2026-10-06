# EBIP-1 standalone physical image envelope

Status: experimental memory-prototype format. No runtime WAL selects these bytes.
This is the complete serialized image-plan limit, not a whole-process heap quota,
a durable commit fence or a PostgreSQL compatibility format. See
[ADR 0048](adr/0048-bounded-physical-image-envelope.md).

## Integer widths and integrity

All integers are unsigned little-endian fixed widths. No alignment padding is
implicit. Bytes have one canonical section order, no optional trailing extension
and no unspecified fields. Unknown versions and nonzero reserved bytes are refused.
A 32-byte SHA-256 over every preceding byte finishes the envelope. The hash and
nested CRCs detect corruption; they provide no authentication or authorization.

## Header (192 bytes)

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 8 | `EBIP` followed by four zero bytes |
| 8 | 2 | envelope version, currently 1 |
| 10 | 2 | header length, exactly 192 |
| 12 | 4 | zero reserved bytes |
| 16 | 16 | nonzero persistent database identity |
| 32 | 8 | exact base transaction, at least 1 |
| 40 | 8 | base transaction plus one, at most `u64::MAX - 1` |
| 48 | 32 | complete base-model fingerprint |
| 80 | 32 | expected complete next-model fingerprint |
| 112 | 4 | changed/appended history page count |
| 116 | 4 | changed root count |
| 120 | 4 | retired table-root count |
| 124 | 4 | aggregate primary upsert count |
| 128 | 4 | aggregate primary retirement count |
| 132 | 8 | total envelope length including the final digest |
| 140 | 52 | zero reserved bytes |

At least one history, changed-root or retired-table record is required. A zero
image-body transaction can still change a root revision, so it is supported.

## Body sections

1. History writes: each existing EBNS-1 address (64 bytes) followed by its original
   EBPG-1 image (4096 bytes). Database/domain/table must equal the envelope's
   database / relational history / zero. Pages are consecutive in ascending ID;
   the exact immutable replay base decides whether their starting tail is valid.
2. Changed roots, sorted by distinct positive table ID. Each contains the existing
   EBIR-1 binding (192 bytes), its primary upsert and retirement counts (two u32s),
   then all upserts (EBNS + EBIX image, 4160 bytes each), then retirement addresses
   (64 bytes each). Each sublist is strictly ordered by page ID; upserts and
   retirements are disjoint. All addresses share this table and database in the
   primary domain. Root owner transaction equals the next transaction. A declared
   predecessor cannot come from after the base; a new root has no retirements.
3. Retired roots, sorted by distinct positive table ID. Each is its exact old
   EBIR binding (192 bytes) followed by its canonical index fingerprint (32 bytes).
   Its owner transaction cannot exceed the base. A table cannot appear in both
   the changed and retired sections.
4. Final SHA-256 digest (32 bytes).

The existing nested address/root/page formats retain their original magic,
versions, checksums and rules. Equal page numbers in different tables/domains do
not alias. No tag reinterprets the existing WAL-1/2 frame bytes.

## Bounds and exact accounting

History / aggregate primary upserts / aggregate primary retirements / changed
roots / retired tables have independent maxima 256 / 2048 / 2048 / 128 / 128.
An individual root has at most 1024 upserts and 1024 retirements. Header totals
must equal the exact sum of bounded nested counts; reaching an input boundary
never substitutes for a declared count.

For counts H, P, D, R and T respectively:

`total = 192 + 32 + (H + P) * 4160 + D * 64 + R * 200 + T * 224`

The loose independent maximum is **9770208 bytes**. Image bodies alone account
for9437184; the remaining333024 cover every address/root/count/header/digest.
Some simultaneous maxima need not be reachable in one valid live table state.
Counts and overflow-safe arithmetic are validated before digest traversal; exact
supplied length must agree with the total. Inputs above the cap are refused.

Decoding first checks the complete SHA, then scans all nested counts, ordering,
scopes and original page CRC/layouts without constructing owned image vectors.
It decodes one bounded page at a time. Only after this preflight does a second
pass reserve bounded image vectors and copy original bodies. Main vector
reservation failures are typed. Tiny nested decoder allocations, OS/resource
failure, decoded relational/index states, shared views, stacks and simultaneous
input/output buffers are outside any allocator quota here.

## Structural admission versus replay

A structurally valid envelope is insufficient to select a state. `ImagePlan::replay`
still requires the exact base database/transaction/fingerprint, preserves every
committed relational slot and canonical append packing, applies original exact
index predecessors and validates full topology/live keys/current row pointers.
The resulting complete state must match the next fingerprint. A public recomputed
hash cannot authorize a different base, pointer or selected projection.

Encoding/decoding writes no files and changes no managed ACK behavior. A future
single-fence writer still requires explicit byte/worker/lifetime admission and
crash, fault, restore, compaction and upgrade evidence. Existing runtime files,
WAL 1/2 and backups do not contain EBIP.

## Compatibility evidence

The synthetic `crates/commit-model/tests/fixtures/empty-rebuild-ebip-1.hex` freezes
one424-byte zero-image root-rebuild envelope. Changes that alter its meaning or
bytes require a new version and compatibility tests; do not silently redefine
version1. This is experimental compatibility evidence, not a stable production
format promise. Conversion must preserve its source and validate a separate copy.
