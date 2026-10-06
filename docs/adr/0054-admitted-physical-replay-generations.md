# ADR 0054: admit destination lifetimes before physical replay

Status: accepted for optional in-memory replay. No runtime WAL selection.

## Context

ModelPool admits registered identities, readers, generations and active writers.
DecodedPlanPool separately admits physical image vector payload. Standalone
ImagePlan::replay returns a raw clonable Model outside ModelPool. Composing these
APIs must not expose an uncharged destination generation or publish into a
foreign equal-state project.

## Decision

Add ModelProject::replay borrowing an immutable AdmittedPlan. Reserve the same
per-project writer exclusion, global writer slot and output generation used by
staging, atomically before any reconstruction. Full existing exact-base physical
replay remains unchanged: database, adjacent transaction, fingerprints, root
predecessors, complete history/index topology and live pointer coverage verify.
No user work runs under the accounting mutex.

Return a non-clonable AdmittedReplay containing the verified Model, exact base
Generation and both leases. Expose only borrowed rows/schema/locations and bounded
metadata; no owned Model or Snapshot conversion escapes. Decoded source lifetime
and its independent vector budget remain separate. After synchronous replay,
output can outlive the source because it owns reconstructed state.

Memory-only publish_replayed requires Arc identity of the exact destination
Generation. Equal fingerprints/database IDs in other pools cannot authorize
publication. Publication transfers the output generation lease, releases writer
exclusion and drops the base when its last reader/descendant releases it. Discard
and conflict drop output storage before advertising the generation slot. Dropping
a project with pending replay keeps its identity registered through that result.
New operations fail closed after poison; cleanup retains existing unwind behavior.

Stages, prepared stages and replay share one global/per-project writer ledger.
Pending results hold exclusion after reconstruction until explicit publication or
discard. The cap applies to admitted lifetimes, not merely active thread stacks;
no new internal worker executor, queue, waiting loop or async engine is introduced.

## Evidence and limits

Four private cases verify actual Weak state/base/owner release, publication
ownership, caller unwind and poison cleanup. Eleven integration cases include
independent replay/parity, stale/foreign bases, equal-state foreign publication,
source/project lifetime, disabled writer, retained-generation refusal, stage/replay
exclusion, Unicode/long keys and root retirement. A 48-case independent row model
checks accepted/discarded histories and old readers. Eight actual threads hold all
successful results while checking the exact two-writer/global-generation cap.

An isolated optional native fixture builds all retained inputs/pools outside the
sample. Refusal by generation exhaustion and existing project writer each observes
zero requested allocation before replay. Accepted 256-row/3072-byte-value replay
observes 1725668 requested retained bytes and 2521808 peak bytes; operation-local
current bytes return to zero after discard. This measures a fixture, not a numeric
model/transient quota. Thread stacks and allocator/profiler overhead are excluded.

Raw Model/ImagePlan APIs remain explicitly unadmitted. Source bytes/vectors and
caller-owned row copies have separate boundaries. Model, cache, staging and replay
heap/transients require numeric reservation before the combined durable writer
can be enabled under ADR 0031. The existing four server workers are not wired to
this coordinator; no durable ACK, file/WAL/backup contract or production gate changes.
