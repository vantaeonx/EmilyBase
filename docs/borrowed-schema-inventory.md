# Borrowed schema inventory

`Snapshot::schema_refs()` visits complete immutable schemas without making
owned copies. Its exact-size, fused, double-ended iterator follows ascending
live table IDs, including gaps after drops. Recreated names appear under their
new IDs. References cannot outlive the snapshot borrow. `table_count()` reads
only the live count; `schemas()` still provides independent owned copies.

Model preparation and validation now use borrowed metadata while preserving
all table/root/type/coverage checks and exact fingerprints. Project status
uses the count directly. Cache warmup retains only bounded names because cache
loading mutates its snapshot; its input budget and filesystem checks still run.
See [ADR0065](adr/0065-borrowed-schema-inventory.md).

A real native regression first fails on 881ca75. With 128 tables and 64 columns,
warmed preparation falls from 1,459,120 to 22,192 requested allocation bytes,
and the peak from 723,160 to 4,784. A separate 1000-pass borrowed inventory scan
requests zero. The cold fixture and index stage precede profiling, and dropping
the prepared state leaves zero tracked bytes. The [source-bound observation](measurements/2026-10-07-borrowed-schema-inventory/operation-allocations.json)
records the dimensions and exclusions. This does not measure throughput, cold
memory, RSS, allocator overhead or whole-process admission.

No format, cache version, SQL, HTTP API, mandatory WAL or durable ACK changes.
Old views keep their original metadata across COW mutation, drops and recreated
names. The complete existing suites remain required. Numeric retained-model
budgets and the combined durable writer are still separate unfinished work.
