# ADR 0041: share immutable relational snapshot tables

Status: accepted. This is per-table copy-on-write, not MVCC or heap reservation.

## Context

The synthetic diagnostics in ADR 0040 show that begin eagerly copies rows and
location keys from every table. Empty/read-only transactions and index-only model
stages pay this cost without changing rows. A small write also copies unrelated
tables. Immutable page images and derived trees already have explicit sharing.

## Decision

Store each private relational table under an Arc, with its schema/row map owned
by that table. Snapshot cloning copies bounded outer metadata and page handles,
and shares immutable tables. Store each per-table physical-location map under
the same kind of Arc. Validated insert/replace/delete detaches the affected table
and locations with safe `Arc::make_mut`. Further writes to the unique table reuse
that private copy. Drop removes the local outer-map reference; creation publishes
a fresh table. No mutable row/schema references escape the snapshot API.

Keep validation and derived-index preparation before state/page/location changes.
Existing rollback/abort, exclusive managed ownership, WAL sync-before-publication,
physical-image checks and immutable-reader behavior remain unchanged. Neither
the database/catalog format nor hashes, durable transaction numbering, API/SDK
shapes or mandatory WAL selection changes. No unsafe block or dependency is added.

Extend the opt-in diagnostic with `--mode index-only`: publish a complete verified
tree selection without a relational event. Count-only version-1 reports accept
this additional mode; previous modes/reports remain accepted. The same state
phase order and encoded-component checks apply. This mode exercises only the
experimental memory model and does not create a durable table index.

## Verification

First reproduce eager row copying with a pointer-identity regression. Verify exact
unchanged-table identity across clone, first mutation, preparation/publication,
rejected events, rollback, retirement/recreation and concurrent independent branches.
Check shared location maps separately, plus old physical images and all value types.
Reach 10000 global rows with eight historical views and a one-row write in another
table. Independent generated row/replay models and managed WAL-1/2 checks cover
commit/abort/reopen/checkpoint/compaction. Existing crash/backup/isolation suites
remain required because the synchronous runtime uses the same Snapshot type.

## Consequences

No-op, read-only, index-only and writes to a small table avoid cloning unrelated
row bodies and location keys. A write still copies its entire affected table and
per-table location map when shared. Original B+ mutation/validation copies also
remain. Outer maps/page vectors allocate, old views retain immutable objects,
and unlimited retained generations still require admission/lifetime policy.
This is a measured reduction for specific shapes, not a hard memory limit.
See [the profile follow-up](../model-allocation-profiles.md#shared-table-follow-up).
