# Durable index prototype: implementation and evidence order

This is an implementation plan for proposed [ADR 0031](adr/0031-atomic-index-wal-proposal.md).
It does not select or enable a new file format. Runtime WAL 1/2, derived primary
trees and optional EBTI caches retain their current behavior.

## Existing building blocks and gaps

`IndexSnapshot` already validates stable arena IDs, topology, root, revision and
entry counts. `SnapshotDelta` binds an exact canonical SHA-256 base, requires the
next revision, validates ordered upserts/retirements and returns a complete new
tree without changing its base. These revisions are standalone index revisions,
not database transaction IDs. Neither object identifies a managed database or
table, and neither participates in its mandatory WAL commit.

Managed replay currently accepts contiguous append-only relational page history
and rejects rewrites of previously committed slots. That invariant must remain
explicit when introducing another page domain: index replacement/retirement is
not permission to rewrite relational history. Root damage cannot be recovered
by adopting an EBTI sidecar or a checkpoint with missing authoritative history.

## Increment 1: typed namespace and root metadata

First specify an experimental, standalone codec with explicit database identity,
table identity, page domain and page ID. Specify root metadata binding key type,
tree revision, exact predecessor, owning transaction, root and covered/excluded
key counts. Do not pack IDs into the current global relational page-number range
or reinterpret EBIX reserved bytes. Select exact integer widths, byte order,
reserved-field rules, checksums and total input limits in a dedicated format ADR.

Implement encode/decode and typed errors before exposing it to runtime recovery.
Acceptance includes every-byte cuts, repaired-checksum invalid domains, zero and
overflow boundaries, duplicate table/index numbers and independent round-trip
models. Equal arena IDs in separate tables/domains must remain distinct. Decoding
must not allocate from an unchecked length or panic on external bytes.

## Increment 2: isolated transaction model

Use an independent map of table rows and selected index images as the reference.
Stage table events, index upserts/retirements and root changes together. Refusal
leaves both previous states unchanged. Validate complete topology and every live
key/current row pointer, including reuse of retired arena IDs. Bind root changes
to the exact predecessor fingerprint and transaction, rather than just an equal
revision number. Cover two tables with equal page numbers and concurrent old views.

Preserve integer and text ordering, exclusion of text keys exceeding the existing
256-byte tree limit, their relational-map path through 3072 bytes and merged LIMIT
semantics. There is no new secondary-index DDL in this increment.

## Increment 3: capacity before writer admission

Current limits include 256 table events/pages per transaction, 1024 index arena
pages, 10000 global live rows and a 64-MiB WAL. A full dense 10000-row rebuild requires
768 index pages. Do not silently use the table-page bound for index images.

Full-capacity integer, 256-byte text and 3072-byte excluded-text model cases now
execute. [Capacity arithmetic and admission gaps](durable-index-capacity.md)
separate existing encoded object sizes from unimplemented WAL/heap budgets.
The model now also admits at most 2048 combined live index pages and exposes
checked standalone component reports; canonical hashes avoid a redundant full
envelope and immutable selections reuse them. See
[ADR 0039](adr/0039-combined-model-index-images.md). Shared heap/WAL reservation
and runtime old-view lifetimes remain open, so increment 3 is not complete.
An optional [ModelPool](model-lifetimes.md) now tests explicit retained/pending
state, reader and writer count reservations; raw models and server/replay
transients remain outside its boundary. This does not select a byte quota.
Raw [physical image plans](model-image-plans.md) also now reconstruct changed
history/index/root components against an exact base and next state, with explicit
separate count bounds. They select no outer wire format and close no writer or
numeric heap/replay gate.

Calculate combined encoded bytes and peak staged/replay memory for a full rebuild,
multi-table transactions, long keys and compaction. Select explicit per-domain
and combined bounds with overflow checks. Verify both full capacity and rejection
before writes. Account for all four current server workers when selecting memory
budgets; per-request bounds alone do not establish a process-wide limit.

## Increment 4: a single durable decision

Only after the preceding format/model/budget gates, add a versioned WAL reader and
writer with table/index/root/retirement tags and one commit fence. Keep old formats
explicitly readable. Stage and validate both projections before append; publish
memory and return an ACK only after the complete commit record is synced.

Execute cuts at every byte, kills around image/commit write, sync, memory publication
and response, plus short/zero/interrupted writes and read/truncate/disk-full/sync
faults. Retain every observed ACK. A complete unobserved commit can survive the
existing response-loss window; an uncommitted transaction must expose neither
projection. Preserve typed poisoned/unknown-outcome behavior and original bytes.

## Increment 5: copies, compaction and compatibility

Extend verified backup/restore and compaction to selected roots and their complete
page domains. Test independent subsequent writes, exact-base refusal and every
publication boundary using the owned-directory machinery. Do not turn optional
sidecar damage into mandatory-state authorization.

Introduce migration only on a verified synthetic copy with a readable old backup,
no-clobber output, explicit unsupported downgrade and frozen old-format fixtures.
Real-data migration, production readiness and completed hardware/security/load
gates require separate evidence and authorization. Update ADR 0031's status only
after its acceptance criteria actually execute and pass.

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
