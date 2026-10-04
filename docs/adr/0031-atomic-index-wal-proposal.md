# ADR 0031: atomic durable table-index WAL integration

Status: proposed. No new runtime format, migration or durable index is implemented.

The [prototype implementation order](../durable-index-prototype-plan.md) separates
namespace codecs, independent transaction models, capacity admission, WAL enablement
and verified migration. This plan does not accept the proposal or close its gates.

## Context

Current table data is authoritative in WAL 1/2. B+ primary trees are derived from
validated live rows; EBTI sidecars are disposable and omitted from backups. Their
successful publication is not part of a table commit. Borrowed reads and bounded
mutation selection now use the original tree, but do not change that boundary.

A separately published index file cannot share an atomic commit with a table
journal merely by ordering fsync/rename. A crash between publications can select
one side only. Durable integration must have one commit decision and prove that
every selected root describes the same table state as its committed row images.

## Proposed direction

Keep the mandatory WAL as the single authoritative commit fence. Prototype a new
version with explicitly tagged table-page, index-page, root-metadata and index
retirement records. Never reinterpret existing EBPG/EBIX bytes or add an implicit
second mandatory journal. Existing WAL 1/2 and their backups remain readable;
current code continues to reject unknown versions. The exact new byte layout is
not selected by this proposal and requires its own format specification/tests.

Give index addresses an explicit table/domain namespace. Equal page numbers in
two tables, or in table and index domains, must never alias. Root metadata binds
the persistent database identity, table ID, key type, selected root, tree
revision/counts and the owning transaction. Stable arena IDs can be reused only
with an exact predecessor revision/base; old snapshots/deltas cannot redirect a
current root through a reused hole.

Stage relational changes and corresponding index changes in one owned
transaction. Before append, validate changed root selections against resulting
live key/pointer images. Sync the complete transaction's commit record before
publishing either state or acknowledging success. A write/sync error retains
existing poisoned/unknown-outcome behavior. No table-only or index-only ACK is
allowed. Recovery selects complete committed states and validates topology,
eligible live keys and current row-image locations before exposing them.

Retain the existing key-admission contract: integer and short text entries in
the tree, long text keys through 3072 bytes in the ordered relational map. Root
counts/coverage explicitly account for excluded long keys. Full ordered reads
merge both sources before LIMIT. This proposal does not reduce accepted key sizes
or add secondary DDL, MVCC, password accounts or a compatibility protocol.

Physical checkpoint/index files can remain disposable materializations. They
must not provide acknowledged history missing from WAL. Mandatory root/page
damage fails closed; silently falling back to an optional cache is insufficient.
Recovery/repair procedures must preserve originals and verify a new destination,
rather than rewrite a damaged source opportunistically.

## Admission, rebuild and compaction questions

The existing 256-event and 256-table-page bounds are separate from index-image
work. A dense rebuild of a valid 10000-row tree takes 768 pages; an arena-exhaustion
fallback cannot assume it fits 256 image records. Select explicit combined page,
byte and memory budgets before enabling durable mode. A valid admitted table
must not lose rows or silently change its key limit because an index is rebuilt.
Capacity refusal before publication must preserve the previous whole state.

Compaction must preserve relational history/location compatibility while keeping
exactly the selected index roots/pages and required predecessor information.
Retiring index images is distinct from relational history vacuuming. Calculate
worst-case journal growth, cold replay memory and concurrent server-worker memory
before choosing limits; today's worker count is not a whole-process memory proof.

Migration must be explicit and initially operate on a synthetic verified copy.
Build/validate complete roots, publish the new-version selection atomically,
preserve a readable old backup and refuse unsupported downgrade. Backup/restore
must replay and validate the new domains together. No real project data is
authorized for this experiment.

## Required evidence before acceptance

- Frozen old-format bytes and read/write/backup compatibility for WAL 1/2.
- Exact table/index/root state comparison against independent integer/text models.
- Every-byte transaction cuts and repaired-CRC/domain/root/pointer corruption.
- Kills before/after image append, commit write/sync, memory publication and ACK.
- Short/zero/interrupted writes, disk-full, truncate/read and sync fault matrices.
- No uncommitted staged state after replay; every received ACK retained, with
  complete committed outcomes allowed in the existing response-loss/unknown gap.
- Exact-base rejection, hole reuse, multi-table namespace isolation and old views.
- Full 10000-row capacity, rebuild/retirement/compaction, long keys and memory bounds.
- Verified restore into a new directory, independent subsequent writes and
  upgrade-publication cuts without modifying the source.

Only then can a new accepted ADR and format document enable the implementation.
This proposal closes no recovery, security, production or durable-index gate.
