# ADR 0050: stream complete index snapshot admission

Status: accepted for standalone indexes. No new durable table-index WAL.

## Context

Snapshot validation/hash materialize all physical pages and reconstruct a second
arena. Delta generation additionally clones/encodes its target just for admission;
application builds all candidate images and reconstructs another owned tree.
These temporaries undermine later memory reservation even when no index page
changes. A full10000-key text fixture reproduces7502704 requested validation bytes.

## Decision

Factor one complete bounded map/topology admission and one original page round-trip
check. Validate exact map/page identity and stable arena domain explicitly rather
than depending on complete import reconstruction to discover them. Stream one
encoded/decoded page through validation/hash/output; allocate only final output
when encoding and borrow fixed validated input chunks when decoding. Keep every
version/CRC/local-layout/topology/coverage check and canonical header/body bytes.
Main output allocation refusal is typed.

Validate delta targets by borrowed reference instead of cloning/encoding them.
Apply upserts/retirements to one private owned candidate map and validate it through
the same complete admission before returning. Preserve exact base digest/revision,
ordered/disjoint bounded images and prior error behavior; a malformed final arena
still refuses atomically. No table model, runtime WAL, backup or ACK decision changes.

## Verification and limits

Before changing index code, an isolated optional native test fails its full-text
validation guard with a7502704-byte requested peak. Afterward validation/hash and
no-op/one-page delta generation observe25376 bytes in one run. Encode/decode/apply
include their required output/candidate; four actual start-coordinated workers
observe96360 transient bytes. Retained fixtures are built outside each profiler;
these guards are workload/toolchain observations, not global allocator quotas.

Frozen independent format hashes, materialized-import refusal parity, generated
arena and independent mutation models,1024-page manual capacity, root collapse,
sparse reuse, exact base/terminal revisions and complete state comparisons execute.
Raw/repaired envelope and generated delta sanitizer targets exercise input admission.
Optional instrumentation stays outside normal binaries and writes no test heap
traces. See [observations and complete boundaries](../streamed-index-admission.md).

At this decision, private candidate map copies still own complete decoded pages.
[ADR0051](0051-shared-immutable-index-pages.md) subsequently shares immutable page
bodies while retaining private maps and complete admission. Retained models,
raw/serialized plan pools, caches, staging and OS stacks require separate admission.
This optimization selects no new format and completes no production/recovery gate.
