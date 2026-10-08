# ADR0101: Bounded policy record groups over original typed pages

Status: accepted as a pure codec foundation; no private catalog installation yet.

A valid policy document can occupy16384 bytes, exceeding the3072-byte value and
4000-byte encoded-record limits. Preserve those engine bounds. Encode one strict
metadata row and up to seven3072-byte payload rows, all ordinary original typed
rows. Exact schema bytes (at most4000) precede exact definition bytes (at most16384).
No external store, larger page, new WAL event or core file format is introduced.

Metadata records canonical decimal table/revision/predecessor IDs, codec version1,
canonical project, separate bounded schema/document lengths and SHA256. The
fixed domain and project/table/revision/predecessor/lengths/body bind the complete
group. Revision must be at least2; predecessor is0 or at least2 and lower. The codec
does not allocate a revision or prove correspondence to a private commit. A future
trusted writer must do that under exclusive ownership and atomically commit every
header/chunk change in the same original transaction.

Chunk keys are exact decimal table ID plus colon plus zero-based index. Require
complete ordered groups, exact canonical keys and exact full/last payload lengths;
reject extra/missing/reordered/duplicate fragments. Validate all declared bounds
before reserving the combined body, compare checksum in constant time, then decode
and validate the original schema and compile the entire strict policy. Recomputed
checksums cannot bypass nested grammar/schema/type/resource constraints. They do
not authenticate a trusted owner's intentional valid policy replacement.

Keep exact definition bytes for trusted explicit comparison, not logs. Encoded/
decoded model Debug and errors redact document/identity. Every metadata/payload row
must fit normal original schemas/pages. Public codec functions do no I/O, clock,
revision allocation, catalog installation, session authentication or authorization.

Acceptance covers an independent Python binary-schema/digest vector, full u64
metadata, maximum document/schema and seven-fragment boundary, strict metadata and
fragment corruption, repaired-checksum nested failures, independent generated
roundtrips/mutations and sanitizer record/document fuzzing. Original WAL1/2 tests
compose complete groups with atomic partial-replacement rollback, reopen, compact,
verified backup/restore and independent clone corruption. Private v1..v3 stores
remain unchanged; explicit future policy catalog versioning/install/revisions,
roles, user filtering/CRUD and HTTP remain separate acceptance gates.
