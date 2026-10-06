# ADR 0048: bounded standalone physical image envelopes

Status: accepted for the memory prototype. No runtime WAL or durable writer.

## Context

Physical ImagePlan already scopes original pages/root selections and independently
replays them against an exact immutable model. Its image-body count omits address,
root, retirement and framing bytes. A future shared journal needs a complete
bounded representation rather than assuming the current4160-byte WAL frames can
be reinterpreted. Count/lifetime admission and observations do not bound heap.

## Decision

Specify the standalone EBIP-1 envelope in
[the complete format](../image-plan-format.md). Use a fixed192-byte header,
explicit adjacent transactions and base/next fingerprints, canonical ordered
sections of unchanged EBNS/EBIR/EBPG/EBIX components, and one SHA-256 trailer.
Every fixed-width length/count participates in checked total accounting.
Independent component maxima yield9770208 bytes, including333024 possible bytes
of metadata beyond page bodies. This cap is serialized length, not a WAL or heap
reservation; no default deployment/process size is inferred from it.

Expose synchronous ImagePlan encode/decode and checked PlanCounts envelope length.
Reject input length/version/identity/reserved/count/digest errors first, preflight
all nested bounds/order/scope/CRC/layout before image-vector allocation, and only
then copy the admitted bodies. Fail main vector reservation with a typed error.
Do not make a structurally admitted envelope a selected state: full existing replay
still verifies exact base, committed-slot preservation, both complete projections,
index predecessors, current pointers and expected next state.

Freeze one synthetic zero-image rebuild frame as hex. Unknown versions fail
closed; incompatible meaning requires a new version and conversion on a verified
copy. Existing managed WAL1/2, raw pages, caches and backup bytes remain unchanged.
No stored version is enabled by adding this independent codec.

## Consequences and verification

Every-byte cuts/damage, extra suffix, repaired outer digest, reserved/unknown
versions, overflow/max counts, nested disagreement, scope substitutions, duplicate
history/root order, retirement overlap and public hash/fingerprint substitutions
exercise refusal. Independent generated row histories verify accepted/discarded
transactions through encode/decode/replay; an independent sum verifies complete
length arithmetic. Nonfirst text primary keys, UTF-8/NUL,256/3072-byte coverage,
128 created/retired roots,768 full-capacity index images and256 changed history
pages preserve separate domain bounds. The frozen424-byte fixture preserves its
original bytes. A dedicated raw/generated parser fuzz target exercises the same
boundary without adding a runtime dependency.

Numeric decoded-state/retained/transient memory, common worker reservations,
single durable fence, migration/compaction/restore, physical power-loss and full
security/load acceptance remain open. ADR0031 stays proposed. An encoded cap must
never be advertised as a whole-process allocator or concurrent replay guarantee.
