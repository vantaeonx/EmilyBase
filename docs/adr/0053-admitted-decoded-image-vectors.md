# ADR 0053: reserve retained decoded physical plan vectors

Status: accepted for optional decoded plan ownership. No runtime WAL selection.

## Context

EnvelopePool reserves serialized EBIP bytes only. A structurally admitted decode
also owns history/index image vectors, typed addresses, changed roots and retired
roots. These retained vectors require their own budget independently of encoded
source bytes, model generations and later replay.

## Decision

Add an explicit DecodedPlanPool with independent plan-owner and vector-payload caps,
bounded to4096 owners and64 MiB. No default picks deployment capacity. Either zero
disables new retention. Full borrowed EBIP preflight precedes reservation; one mutex
atomically reserves the owner and complete typed vector payload before construction.
No user work or decoding occurs inside that critical section.

Compute payload from checked PlanCounts and this binary's size_of values for all
history/primary PageWrite elements, retired PageAddress elements, RootChange
elements and RetiredRoot elements. This includes padding and inline child-vector
headers inside each owned root element. It is a host-layout memory accounting
rule, not a serialized field, portable ABI or complete heap formula.

The decoder still uses its original complete structural preflight and exact-length
reservations. Before returning, compare actual vector capacities against the full
reserved payload. Unexpected spare capacity refuses publication; private vectors
and their permit drop. This prevents a successful retained object silently owning
unused vector elements outside its charge. The optional guard does not alter the
unadmitted ImagePlan codec's format or its public standalone behavior.

AdmittedPlan owns one immutable Arc-backed plan. Clone shares the same actual
vectors and reservation; independent decodes reserve independent owners/copies.
Borrowing a plan cannot convert its vectors into owned uncharged output. Last-owner
field drop frees vectors before releasing their permit. New operations fail closed
after internal mutex poison; exactly-once cleanup still recovers the accounting
lock during unwind. Existing immutable shared handles remain usable.

AdmittedEnvelope::decode_in retains the source's independent serialized charge.
Decoded images can outlive that source and its pool handles. Structural decode
does not authorize a database/base or prove complete row/index state: exact replay
against a matching immutable model remains mandatory and unadmitted model output
is explicitly outside this pool.

## Verification and limits

Exact bytes/one-byte-short, mixed copies, shared pointer identity/last release,
source independence, foreign/stale base refusal, actual4096 owners and two actual
eight-thread reservation races execute. An independent64-case owner/handle model
predicts charges and verifies every retained plan. Private shape regression,
poison/unwind/Weak release and maximal arithmetic permit checks exercise cleanup.

The isolated optional native diagnostic holds source/fixtures/pools outside its
operation sample. A256-history-page plan reserves1228576 vector bytes and observes
1228776 current/peak requested bytes:200 additional owner/control bytes excluded
from vector accounting. Disabled decode observes3292 transient bytes and owns no
image vectors; four shared handles allocate no additional bytes. Final operation
current bytes return to zero. These are workload observations, not complete quotas.

Arc/plan/permit inline allocation, ledger metadata, allocator rounding/overhead,
source buffers, per-page validation scratch, stacks, caller-owned handle containers,
raw plans constructed elsewhere, decoded row/index models, caches, staging and
replay output/transients remain outside this cap. Capacity mismatch refusal can
hold private unsuccessful allocations until cleanup; this is not an allocator-wide
hard peak guarantee. The combined durable writer remains gated on complete model/
transient/worker reservation and recovery acceptance under ADR0031.
