# Streamed standalone index admission

Standalone EBIF validation, canonical hashing, encoding and delta application now
share complete bounded topology/map identity and original per-page wire checks.
They do not materialize an intermediate complete image set merely to validate
an already owned tree. See [ADR 0050](adr/0050-streamed-index-snapshot-admission.md).

## Preserved admission

Snapshot revision must be nonzero and its arena must use stable IDs. The map has
1..1024 pages; every map key equals its embedded page ID in that same bounded
positive domain. Full tree validation still checks local keys/pointers/arity,
occupancy, balanced depth, cycles/shared children, exact separators, reachability,
leaf links and complete entry count. Each original page still encodes and decodes
back to the exact owned value, using its unchanged local version/CRC/layout rules.

Validation/hashing hold one4096-byte image and its decoded local scratch at a time.
Encoding allocates only its final bounded EBIF vector plus that scratch; the main
output reservation can return a typed allocation error. Decoding borrows fixed
image slices from its checked immutable input instead of copying a full image Vec,
then returns an independently owned decoded tree.

Delta generation validates its borrowed target without cloning a complete tree
or encoding a discarded full EBIF envelope. It emits only changed upserts and
retirements against the exact original fingerprint/revision. Delta application
still clones a private candidate map, applies bounded ordered/disjoint changes,
and fully validates the complete candidate before returning it. It does not encode
all candidate pages into a Vec and reconstruct a second complete tree. Every
failure preserves the base. No internal invalid map can bypass final topology
merely because each supplied upsert page decoded correctly.

## Reproduced allocation regression

The isolated opt-in index_stream test builds a10000-key,768-page,256-byte-text
fixture before profiling each individual operation. Retained fixtures are outside
those counters. The old validation fails its128-KiB transient assertion with a
requested peak of7502704 bytes. The updated implementation passes; one observed
native run records these operation-local requested peaks:

| Operation | Requested peak bytes |
| --- | ---: |
| Complete validation | 25376 |
| Canonical fingerprint | 25376 |
| Encode, including its final3149824-byte output | 3154096 |
| Decode, including its owned result | 3308400 |
| Unchanged-target delta generation | 25376 |
| One-page pointer-change delta generation | 25376 |
| Apply, including its private owned candidate | 3307848 |
| Four actual scoped workers over independent retained fixtures | 96360 |

The four workers share a start barrier and all joins complete; OS scheduling can
serialize parts of their work. Their test uses a512-KiB transient guard. Serial
validate/hash/delta guards are128 KiB; encode permits the complete output plus
that scratch, while decode/apply permit encoded length plus1 MiB. All measured
operation-local current bytes return to zero in that run. Values can differ with
toolchains, allocator or scheduling; these are regression guards for this fixture,
not whole-process/RSS, stack, fragmentation, server or worst-case heap quotas.
The fixture, profiler bookkeeping and any previously untracked allocations are
excluded. The allocator is linked only to the feature-gated native test process.
Normal server/CLI use neither this allocator nor its optional dependency.

## Compatibility and refusal evidence

Frozen independent EBIF SHA fixtures and original EBIX bytes remain unchanged.
Private malformed maps compare streaming refusal with the former materialized
import oracle. An independent64-case valid/corrupted arena model and48-case mixed
key mutation model cover admission and accepted/discarded deltas. Sparse root
collapse/hole reuse, exact predecessors, terminal revisions and base preservation
execute. An independently packed1024-page arena with954 leaves,64 lower branches,
five upper branches and one root reaches the exact physical arena cap while
retaining6678 rows and a valid one-page delta. Full10000-row and256-history-image
model/envelope cases remain separate from this index-only capacity shape.

The dedicated index_delta_sequences target additionally verifies generated
mixed integer/text/NUL/256-byte histories, wrong fingerprints/revisions/counts,
complete byte/hash round trips and unchanged historical bases. Existing raw and
CRC-repaired index_snapshot fuzzing still exercises untrusted complete envelopes.
Stored EBIF/EBIX, managed WAL1/2, backup and EBIP bytes and durable ACKs retain their
existing meanings. Full decoded/transient/worker budget and the combined durable
writer remain open; ADR0031 stays proposed.

[The preserved count-only operation sample](measurements/2026-10-06-index-stream/operation-peaks.json) records the quoted local run. It contains no heap stack traces, host paths or input data.
