# ADR 0049: admit retained serialized image payloads

Status: accepted for the memory prototype. Full heap/server/WAL admission open.

## Context

EBIP bounds one complete encoded plan, but a caller can hold arbitrarily many
serialized outputs. ModelPool counts generations/readers/writers without exposing
raw states; raw ImagePlan/Vec outputs have separate lifetimes. A per-envelope cap
cannot enforce a global live payload cap or charge a later clone.

## Decision

Add an optional synchronous EnvelopePool with explicit immutable object/byte limits.
Use checked atomic shared-ledger reservation before serialized output allocation.
No automatic defaults choose a deployment budget. Configuration is bounded at4096
objects and64 MiB of payloads; zero disables access. This cap deliberately counts
complete serialized vector lengths only, not allocator/RSS or decoded model sizes.

Owned AdmittedEnvelope values carry private non-cloneable permits, expose borrowed
bytes and offer fallible admitted cloning. Drop vector contents before releasing
counts. Encoding/copy failure and unwinding release reserved space; poisoned public
operations fail closed while private destruction still cleans up. Handles share
one ledger. Borrowed encoded input is structurally preflighted before copying;
exact-base/state replay remains independently mandatory.

Allow admitted preparation to serialize through this pool without exporting raw
Model/Snapshot or releasing its writer/generation prematurely. Its temporary raw
physical plan remains outside this specific output quota. Model registration does
not become indefinitely pinned by a standalone serialized copy. Full process
admission must account for each remaining allocation class separately.

## Consequences and verification

Boundary/mixed-size/configuration/source/copy/refusal/release cases and a64-case
independent reservation model verify exact live payload/object counts. Two real
barrier-coordinated eight-thread tests hold winners while the coordinator observes
the exact object or byte boundary. Real4096 small buffers exercise full count
capacity. Poison/failure/unwind/weak-ledger tests exercise cleanup. Six maximal
permits fit the hard byte cap and a seventh refuses before materialization; this
arithmetic test does not allocate six maximal models or prove a process limit.
A bounded independent sanitizer target checks the same retained/copy histories.

The pool does not reserve source/raw plan buffers, transient decoder/replay states,
model caches, user-owned copies, vector-capacity rounding, fragmentation, OS stacks
or HTTP/WAL/backup workers. These are explicit remaining gates, not inferred costs
from diagnostic samples. No runtime format/version, fsync, commit fence or managed
ACK behavior changes. ADR0031 remains proposed and production use remains barred.
See [the ownership boundary](../image-buffer-admission.md).
