# Retained physical envelope admission

The optional synchronous `EnvelopePool` admits owned EBIP-1 payload buffers.
It reserves their exact complete serialized length and one object slot before
output allocation. This is a byte quota for those payloads only, independent of
ModelPool generation counts, raw ImagePlans and decoded replay state. It is not
an allocator, RSS, server-worker or durable-journal quota. See
[ADR 0049](adr/0049-admitted-serialized-image-buffers.md).

## Ownership and reservation

The caller explicitly supplies `EnvelopeLimits::new(buffer_count, payload_bytes)`.
Hard configuration bounds are4096 objects and64 MiB of combined payload bytes;
either zero disables retention. There is no default deployment recommendation.
A mutex protects checked additive reservation of both resources in one decision.
Refusal leaves both counters unchanged. No page encoding, copying, validation,
user callback or destruction occurs while holding that mutex.

`encode(&ImagePlan)` checks exact complete envelope length and reserves before
creating the encoded output vector. The borrowed raw plan already exists outside
this pool. `copy_encoded(&[u8])` checks the entire immutable input with EBIP's
borrowed preflight, then reserves before copying it. Preflight's small per-page
decoder scratch and caller-owned source buffers are outside payload accounting.

`AdmittedEnvelope` exposes only borrowed bytes. It does not implement Clone or
return an owned Vec. `try_clone` must reserve another object and its whole length
before copying into a separate vector. A caller can explicitly copy borrowed
bytes independently; those external allocations are caller-owned and unadmitted,
as with copied rows from ModelReader. This library cannot constrain arbitrary
caller allocations or prevent source buffers from being retained separately.

Every envelope privately owns exactly one non-cloneable permit. Its vector field
drops before the permit advertises space as free. Failed encoding/copy and unwinding
release the reservation. Public operations refuse a poisoned ledger; private drop
still recovers the guard and releases exactly once. Cloned pool handles use the
same ledger. The last envelope keeps its ledger alive even after pool handles drop.

## Prepared model integration

`AdmittedPrepared::encode_in(&EnvelopePool)` creates an owned admitted envelope
without exposing a clonable Model/Snapshot. It borrows preparation; serialization
or capacity refusal does not release its active writer/future-generation leases.
Memory-only publication remains a separate operation. Dropping model owners/readers
can release every model generation while an independent encoded payload remains
charged in its buffer pool. The payload itself does not pin model registration.

This method currently materializes a temporary raw ImagePlan before reserving the
serialized output. That plan, retained model state, derived caches, topology
validation, decoder outputs and OS stacks require their own later reservation.
The pool charges vector length, including framing, not allocator capacity rounding,
Vec metadata, fragmentation or profiler overhead. It must never be presented as a
complete transient or whole-process memory proof.

## Verification and boundaries

Exact-byte/one-byte-short, mixed-size, clone refusal, disabled/overflow configuration,
malformed copies, source independence, pool-handle lifetime, real4096 retained
small copies and last release execute. Eight actual threads hold winning outputs
behind barriers: one test admits exactly two object slots; another admits exactly
three424-byte outputs beneath the fourth-byte boundary. An independent64-case
sequence predicts every live byte/object charge across encode/copy/clone/drop.

Private tests inject internal poison, failed materialization after reservation,
unwinding and final shared-ledger release. Six maximal-length permits fit64 MiB;
a seventh is refused before allocation. That last case uses real permits without
maximal payloads and proves reservation arithmetic, not six simultaneous maximum
replay heap workloads. The independent envelope_admission sanitizer target checks
bounded histories, malformed copies and unchanged base state.

Existing managed files, WAL1/2, backup bytes, server admission and durable ACKs
remain unchanged. No whole-stage acceptance or production gate is closed.
