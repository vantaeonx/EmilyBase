# Bounded schema checks without a temporary heap

Catalog schema/key validation preserves table/column identifier, duplicate,
1..64 count, primary type/nullability, row shape and value-size rules. It now uses
fixed arrays of borrowed bounded names plus original positions instead of a
per-call heap set. Duplicate flags are checked in original input order, preserving
which error appears first. Oversized names never enter sort comparisons.

A real native regression first fails on46317ba. For1000 paired checks, one/two-
column schemas request208000 allocation bytes and64-column schemas2272000; repaired
samples request zero. Two1000-call physical-row samples also fall from104000 to
zero. Caller fixtures and names precede profiling; allocator overhead/rounding,
stacks and profiler data are excluded. This does not measure throughput, total
stack usage, cold memory or process quota. See [observations](measurements/2026-10-07-stack-schema-validation/operation-allocations.json)
and [ADR0064](adr/0064-bounded-stack-schema-validation.md).

All schema validation still executes. Inputs remain immutable, and schema/API/
file/WAL/cache bytes and durable ACK retain their meanings. Numeric model/cache/
staging/transient admission, combined durable writer and production gates remain.
