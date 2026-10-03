# ADR 0009: Bounded original B+ tree foundation

Status: implemented and tested as a standalone library; table/WAL integration pending.

## Context

Tables currently rebuild standard primary-key maps from their committed event
history. An ordered page index needs original routing and split logic, a bounded
untrusted codec and structural validation before it can join durable transactions.
Existing managed recovery and journal compaction checks provide the storage
foundation; they do not prove durability for this new index.

## Decision

Use an original B+ tree with linked leaves, exact right-subtree-minimum separators
and fixed 4096-byte `EBIX` images. Signed integers sort numerically; text sorts by
UTF-8 bytes. Mixed key variants use the catalog ordering (integer before text).
No database-engine or third-party tree implementation is used.

For this first bounded implementation, cap text keys at 256 bytes and every node
at 14 keys. The worst-case leaf then fits without variable-size split heuristics.
Non-root nodes have at least seven keys. An arena holds at most 1024 pages and
10000 entries; page capacity may be reached before the entry limit. Depth is
bounded to eight. Input validation precedes decoder allocation.

Copy the bounded tree before insertion; publish the staged copy only after all
splits and allocations succeed. This deliberately favors simple atomic failure
semantics over throughput. The standard map is only a page-ID arena, not the
ordered key index. A future copy-on-write/page-cache strategy requires its own
durability and concurrency checks.

Import requires dense ordered page IDs, known versions, checksums, local layout,
unique child ownership, no cycles, no unreachable pages, correct separators,
non-overlapping child ranges, equal leaf depth and an exact leaf successor chain.
Opaque record pointers identify a page/slot; this library cannot prove they refer
to a live table row. Table integration must validate their ownership and lifetime.

## Consequences and remaining acceptance

The library performs insertion, exact lookup, ranges and page-image export/import.
Its maintenance extension adds replacement, deletion/merges and sorted bulk
loading under the unchanged codec; see [ADR 0010](0010-index-maintenance.md).
It is not a durable table index. It does not change existing database/WAL formats,
silently index existing keys or reject existing table text values over 256 bytes.
Durable root metadata and concurrent readers remain pending.
A catalog/page-kind compatibility decision and atomic WAL replay of
splits/root changes are required before enabling real table index mutations.

Tests compare generated operations against an independent sorted map, exercise
multi-level splits and actual capacity, and reject corrupted images and invalid
topology. Bounded ASan fuzzing exercises raw and checksum-repaired page images.
None of these checks close the project's power-loss or production acceptance gate.
