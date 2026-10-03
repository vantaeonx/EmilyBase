# ADR 0002: Page-backed B+ tree indexes

Status: accepted design; standalone tree foundation implemented, durable table integration pending.

Use an original B+ tree for ordered indexes and primary keys. Fixed-size internal
and linked leaf pages allow point lookup and range scans. Splits and merges must
participate in WAL transactions. A hash-only index was rejected because it does
not serve ordered range scans. Do not implement tree mutation before recovery.

The first bounded page codec/tree implementation follows the managed recovery
foundation. It does not yet publish index pages through WAL or replace table key
maps. Its interim bounds and integration gates are documented in
[ADR 0009](0009-bounded-index-foundation.md).
