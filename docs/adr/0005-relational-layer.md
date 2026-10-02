# ADR 0005: Typed catalog and append-only table events

Status: accepted.

Keep schemas/values/codecs in `catalog`, pages in `storage`, and table coordination
in a separate `database` crate. This dependency direction avoids a catalog/storage
cycle and keeps networking independent. Use bounded original binary codecs;
Serde handles explicit JSON exchange, never the database storage implementation.

Initially persist table changes as individually bounded events in slotted pages
and reconstruct ordered primary-key maps with standard-library BTreeMap on open.
This offers working table semantics before an on-disk B+ tree exists. It is not
the planned index implementation and does not claim scalable query performance.
The initial engine needs explicit limits on live tables, rows and event history.

A root marker distinguishes table files from raw page files. Initialize the
marker before publishing the file, and refuse raw CLI mutations of managed table
files. Reject malformed event sequences on open without repairing or modifying
them. Append events only after validating the requested operation.

Consequence: the last data page is still rewritten in place. Until a real WAL
and its crash tests exist, changes are not transaction-safe and no acknowledged
transaction durability is promised. Record history must not be advertised as WAL.
