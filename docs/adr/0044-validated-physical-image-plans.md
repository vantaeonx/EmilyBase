# ADR 0044: validated physical image plans

Status: accepted for the memory prototype only. No new WAL/file format is selected.

## Context

The transaction model validates whole table/index states, but a durable writer
will eventually need a concrete bounded set of changed physical components. A
root header alone cannot prove that its images reconstruct the same live rows.
Image-domain admission must remain separate from existing table-event admission.

## Decision

A raw Prepared retains its immutable exact previous state and can materialize an
ImagePlan. The plan binds database identity, adjacent base/next transactions and
both complete model fingerprints. It owns original 4096-byte EBPG/EBIX images,
typed PageAddresses, changed root bindings and exact retired root witnesses.
There is no outer binary envelope, record framing, write, sync, ACK or recovery
selection in this API. Existing WAL 1/2 readers still reject unknown versions.

History output includes only a changed last base page and appended pages. Replay
permits extension of that last page only if every previously committed slot is
byte-identical, then accepts contiguous new pages. Earlier rewrites, gaps, duplicate
addresses and mismatched database/domain/payload IDs refuse. Existing original
page decoding and relational event replay validate checksums, slots and catalog.

Unchanged root selections are omitted. Changed existing trees reuse the original
SnapshotDelta construction/replay and bind their exact predecessor. New tables
require revision one and a complete original stable-ID arena. Every index image
and retirement is scoped to database/table/primary domain and sorted unique IDs.
Dropping a table requires its exact old root binding and canonical index hash;
same-name recreation uses a new table ID. Reconstructed selection validates every
live key/current physical row pointer, including long-key exclusions, before
matching the complete expected next model fingerprint. The supplied base never
changes, whether replay succeeds or fails. These public integrity hashes are not
credentials or MACs.

PlanCounts checks separate maximums of 256 history upserts, 2048 primary upserts,
2048 retired index IDs, 128 changed roots and 128 retired tables. Checked image
body bytes have an inclusive maximum of 9437184. That is only the sum of original
page bodies; address/root/retirement/fence framing and allocator costs are excluded.
It is not a WAL-size or heap reservation. Per-tree topology/1024-ID constraints
and complete-state coverage remain independently mandatory.

The plan API belongs to raw memory models. ModelPool exposes no raw Prepared or
image-plan/replay escape. Owned plan buffers and replay outputs/transients need
separate admission before any future server integration. The model now explicitly
uses its original storage crate for physical history decoding; no external engine
or third-party version change is added.

## Consequences and verification

Tests reconstruct equal-numbered pages in two tables and the history domain,
index-only zero-body changes, old views, table retirement/recreation, long keys
and independent accepted/discarded row sequences. Every byte of a real history
and index image is damaged separately. Valid repaired checksums cannot authorize
old-slot/earlier-page rewriting or another table's row pointer. Exact base, adjacent
transaction, complete next fingerprint, ordering, retirement and missing-image
checks reject partial/foreign selections.

A real 10000-row fixture first exposed an incorrect assumption that incremental
index maintenance has the dense rebuild shape: it produced 792 pages. The fixture
now explicitly builds the original dense 768-page arena before reversing its page
IDs/references. The valid readdressed state changes all 768 index images and
replays without reducing the independent 256-history-page bound. Another fixture
replays exactly 256 newly appended pages with 3072-byte values. This is capacity
and memory-replay evidence, not durable/crash or throughput acceptance.

Replay with history changes reconstructs the entire bounded history/catalog in
memory and may copy large page/row structures. Existing delta construction also
validates/materializes temporary full tree envelopes. Both need measurement and
reservation/optimization; no process-wide byte claim follows from image counts.
Numeric heap/transient/replay budgets, a selected wire format, a single durable
fence and complete crash/backup/migration gates remain open under ADR 0031.
