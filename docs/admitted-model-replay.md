# Destination admission for physical model replay

The optional ModelProject::replay accepts a borrowed AdmittedPlan and reserves
writer exclusion plus its complete output-generation slot before reconstruction.
It returns AdmittedReplay, which retains those leases through publication/discard.
See [ADR 0054](adr/0054-admitted-physical-replay-generations.md).

## Composing independent reservations

Serialized EnvelopePool bytes, decoded DecodedPlanPool vectors and ModelPool
lifetimes have independent ledgers. Decoding does not authorize a base; destination
replay still checks complete physical history/index state against exact database,
transaction, fingerprint and root predecessors. Raw replay remains available for
explicitly unadmitted experiments.

ModelPool counts distinct retained/current/pending generations. A reader of the
current state shares its generation; retained old readers can exhaust capacity
for the next one. The same atomic decision checks per-project/global writer
exclusion and the pending generation before replay. Failure changes no current
state and releases all private destination permits. Decoded input remains valid.

A pending result is non-clonable and exposes borrowed rows, schema, row locations,
root bindings, component counts, transaction and fingerprint. It cannot release
its reservation through an owned raw-state conversion. It owns reconstructed
state independently of its encoded/decoded source, which may be dropped afterward.
Its exact base keeps the project namespace and old state alive after owner removal.

publish_replayed performs memory-only publication into the exact base Generation
instance. Equal-state projects in separate pools refuse each other's results.
Publication transfers the generation permit before releasing writer exclusion;
old readers continue to observe their original rows and pointers. Discard/conflict
frees output state before returning its generation slot. Caller unwind also cleans
up; poisoned accounting refuses new work while retained output remains readable.

## Concurrency and memory boundaries

Existing limits remain explicit: up to 128 identities, 4096 distinct generations/
readers and four writers. Replay uses the existing writer ledger along with staged
and prepared writes. Pending output holds its writer slot until publication or
discard; this is stronger than counting only currently executing threads. No
background threads or queues are added. Actual thread tests synchronize start,
held successful output, accounting checks and release, observing the exact cap.

The native operation fixture retains 256 rows with 3072-byte text values. Both
refusal paths observe zero requested allocation before any replay projection.
Successful replay observes 1725668 retained requested bytes and 2521808 peak bytes,
returning operation-local current bytes to zero after discard. Retained source/
models/pools precede profiling; stacks, allocator overhead and profiler bookkeeping
are excluded. These observations do not reserve model/cache/transient bytes.

This remains an in-memory prototype. Runtime HTTP/WAL workers, single durable
commit fence, new-format recovery/backup/migration and production acceptance stay
separate. The next gate is numeric model/cache/staging/replay-transient admission,
not enabling durable mode from lifetime counts alone.
