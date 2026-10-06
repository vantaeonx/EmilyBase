# Durable index capacity: measured objects and admission gaps

This records the current in-memory prototype's capacity evidence. It does not
select WAL 3, enable a durable writer or establish a heap/process memory limit.
See [the implementation order](durable-index-prototype-plan.md) and
[ADR 0038](adr/0038-staged-table-index-model.md).

## Existing object arithmetic

The current global live-row limit is 10000 across all tables, not per table.
There can be 128 tables, 256 relational events per normal transaction and 1024
live pages per individual index arena. All page images are 4096 bytes. EBIF-1
adds one 4096-byte header to the live index pages. Sparse arena IDs do not add
empty image slots: use live page count, not the maximum selected page ID.

| Existing object | Exact encoded bytes |
| --- | ---: |
| Empty one-page primary EBIF | `(1 + 1) * 4096 = 8192` |
| Dense 10000-key, 768-page EBIF | `(768 + 1) * 4096 = 3149824` |
| Maximum 1024-page individual EBIF | `(1024 + 1) * 4096 = 4198400` |
| EBNS-1 typed page address | `64` |
| EBIR-1 selected-root metadata | `192` |
| 128 selected EBIR-1 roots | `128 * 192 = 24576` |

These are existing standalone objects, not a combined WAL encoding. Adding
an EBNS record to an EBIX page yields 4160 bytes before any transaction record
envelope, digest, sequence or retirement record. The coincidentally equal
current WAL-1/2 frame length, 4160, already has different field meanings and
cannot be reused by appending another address to its existing layout.

The current relational WAL reserves `(page_count + 1) * 4160` bytes for page
frames and its commit fence before append. At 256 pages this is 1069120 bytes;
the file header is 64 bytes and the whole file is bounded to 64 MiB. This is
current WAL-1/2 arithmetic. A shared writer must calculate its own complete
transaction reservation and available journal space with checked arithmetic.

## Full-capacity model checks

`crates/commit-model/tests/capacity.rs` constructs real synthetic staged states,
in batches of at most 256 events, up to the global 10000-row boundary:

- Integer keys: complete dense index-only rebuild selects 768 pages while an
  untouched second table keeps its prior root. Old views retain exact images.
  Row 10001 is refused in either table without changing the selected state.
- Exactly 256-byte text keys: the complete dense tree also uses 768 pages.
  Every one of the 10000 keys resolves to its current physical row pointer and
  value after preparation/publication. The old view remains unchanged.
- Exactly 3072-byte text keys: all 10000 keys are explicitly excluded from the
  short-key tree. Its one-page root selects zero entries and 10000 exclusions;
  the relational path still returns every row and physical location. The next
  insert is refused atomically. Direct B+ lookup rejects these oversized keys.

The dense image exceeds the 256-event bound. Its successful model publication
proves that the model does not mistake the event limit for the index page limit.
The model neither appends that image to WAL nor acknowledges a durable commit.
This matrix does not cover all mixed-table fragmentation or long history shapes.

## Bounds required before enabling writes

Independent maxima are insufficient. For example, 128 times the individual
maximum EBIF is 537395200 bytes (512.5 MiB). This is a loose arithmetic upper
bound, not a claim that such a state is reachable with 10000 live rows. It also
does not include relational state. Retaining a base and candidate set can double
encoded objects; four concurrent workers can multiply that further. Those sums
are still not Rust heap measurements: decoded keys/rows, map nodes, capacities,
Arc metadata and encoding/validation copies have additional costs.

An admission design must separately account for:

1. Relational events and changed history images, complete index upserts, typed
   retirements, changed roots and the single commit fence.
2. Exact combined encoded bytes, checked length/counter arithmetic and journal
   capacity, before any append or allocation from external lengths.
3. Retained live state, private relational clones, old immutable views, candidate
   trees, page-image vectors, full encodings and topology-validation rebuilds.
4. Cold replay's retained history and decoded projections, archive/compaction
   construction and work concurrent with the four server workers.
5. Rejection before writes, including a late oversized candidate after earlier
   accepted changes; exact prior table/index state and file bytes must survive.

At the preceding checkpoint, `IndexSnapshot::encode` materialized images,
revalidated a reconstructed arena and allocated a complete EBIF buffer.
`fingerprint` hashed that encoding; model preparation hashed every selected index,
including unchanged roots. `begin` clones the relational snapshot. These are
explicit transient allocations; the follow-up below removes some of them.
An encoded-byte cap alone would not bound them or arbitrarily retained old views.

No combined numeric budget is selected here. A later implementation must first
measure representative and adversarial staged/replay allocations, address retained
view lifetimes and decide how shared worker admission reserves/relinquishes memory.
It must preserve full valid row capacity, then pass independent arithmetic,
overflow, boundary, mixed-table and refusal tests. Existing optional EBTI cache
admission is a separate mechanism and cannot authorize mandatory WAL state.

Recovery cuts/kills, one synced fence, backup/compaction/migration and physical
power-loss/security/load gates remain open after this capacity checkpoint.

## Follow-up: combined image bound and canonical hash reuse

[ADR 0039](adr/0039-combined-model-index-images.md) now implements a 2048-page
aggregate bound for staged candidates and complete selected model states. It
derives a loose 1760-page bound from existing occupancy, 10000 global rows and
128 roots. Full 128-table capacity selects 1536 fragmented pages. Reported EBIF
objects are capped at 8912896 bytes plus 24576 root bytes; those component caps
do not cover history or heap. Earlier loose per-object arithmetic remains a
description of why independent maxima were insufficient.

Fingerprinting now streams admitted page images/header, and immutable selections
cache their validated canonical hashes. `encode` still constructs a full EBIF
buffer; both paths still materialize images and reconstruct topology. Preparation
reuses admitted hashes for unchanged roots instead of re-encoding them. `begin`
still clones relational state and old views can still retain states indefinitely.

The later [shared-table change](adr/0041-shared-relational-snapshot-tables.md)
keeps immutable tables/location maps under Arc. Snapshot cloning now copies outer
metadata/page handles; first write clones only its affected shared table/map.
The preceding allocation descriptions are historical checkpoints. This reduces
no-op/index-only/unrelated-table costs but does not bound retained generations
or writes to a large table.

Before this allocation change, the uninstrumented stable debug capacity binary's
three serial cases passed in 59.68 seconds with a Linux maximum RSS of 224496 KiB
(about 219 MiB). This is one whole-test-process observation on this machine,
including decoded states/retained views/test allocations. It is not a worst-case
heap proof, a throughput benchmark or a four-worker server reservation.

## Follow-up: requested-allocation measurements

The opt-in [release diagnostic](model-allocation-profiles.md) now records actual
allocation counters on bounded synthetic shapes. Four held 10000-row models with
3072-byte keys and 768-byte values peak at 985295630 requested bytes; after releasing
their old views, current requested bytes fall from 985283384 to 572932520. This
confirms substantial relational clone/retention costs independently of image caps.
Construction/publication remain serial, and this is not a worker or worst-case
reservation. Full-versus-streamed hashes match while streaming reduces measured
allocation traffic. Heap admission, lifetime management and WAL/replay budgets
remain open. See [ADR 0040](adr/0040-opt-in-model-allocation-diagnostics.md).

## Follow-up: complete standalone image envelope

[EBIP-1](image-plan-format.md) now accounts for complete changed physical components
including addresses, roots, retirements and its outer digest, bounded to9770208
bytes. Nested preflight precedes owned image-vector construction; complete exact
model replay remains mandatory. This is serialized length admission only. Numeric
heap/lifetime/worker reservation and the combined durable writer stay open under
[ADR 0048](adr/0048-bounded-physical-image-envelope.md); old runtime formats stay
unchanged and no stage acceptance is marked complete.

## Follow-up: retained serialized payload reservation

The optional [EnvelopePool](image-buffer-admission.md) now atomically reserves
complete EBIP byte lengths and buffer slots before encode/copy, including admitted
clones. Admitted preparation can serialize without releasing writer/generation
leases or exposing raw state. Raw plans, decoded/retained model state, temporary
validation and whole-process/server/WAL admission remain outside this scope;
[ADR 0049](adr/0049-admitted-serialized-image-buffers.md) closes no durable gate.

## Follow-up: streamed standalone index verification

[Complete index admission](streamed-index-admission.md) now checks bounded map/page
identity, topology and physical round trips one page at a time. Fingerprints and
wire bytes stay frozen. Borrowed target validation and direct private-candidate
admission remove redundant complete images/trees in delta generation/application.
Isolated full-capacity allocation guards and independent corruption/state models
execute under [ADR 0050](adr/0050-streamed-index-snapshot-admission.md). Candidate
map/retained/transient/worker quotas and the shared durable writer remain open.

## Follow-up: shared decoded page ownership

[ADR0051](adr/0051-shared-immutable-index-pages.md) now keeps immutable index pages
shared across clones/candidates/historical views. Each map remains private; changed
leaves/branches and dense renumbered links detach before publication. Owner/key
identity and Weak release checks execute alongside full original admission.
Full-capacity isolated and four-project replay observations include map/reference
overhead and preserve their exact boundaries in [the report](shared-index-pages.md).
Sharing does not close decoded/transient/worker byte reservation or durability.

[ADR0052](adr/0052-validated-shared-primary-export.md) now also removes full physical
image reconstruction from primary export while retaining complete arena and exact
relational coverage checks. Warm-cache regression and same-config observations
are preserved separately; decoded plan/model/transient/worker quotas stay open.
