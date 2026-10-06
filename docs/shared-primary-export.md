# Validated shared primary export

Relational primary export now returns a fully admitted private stable-ID map
sharing immutable page bodies with the derived index cache. It no longer creates
a complete set of physical images and reconstructs an independent arena merely
to change allocation/deletion policy. See [ADR0052](adr/0052-validated-shared-primary-export.md).

BPlusTree::to_stable preserves exact IDs, sparse holes, root, row count, key/pointer
bodies and original page bytes. It checks the entire bounded map/topology and
every original per-page encode/decode round trip before returning. The source keeps
its original policy; future changes detach private bodies. Bad identities/counts
or topology refuse without repairing, normalizing or altering the source.

Relational export additionally verifies every eligible live key/current pointer
against its row location and resolves that location in the exact snapshot. Text
keys beyond256 UTF-8 bytes retain their existing exclusion/fallback behavior.
Installation still performs full coverage verification. Four independent snapshot
writers share untouched keys and keep their own current row values/pointers.
Retained projections stay valid against their corresponding historical snapshot.

## Operation observation

The isolated feature-only native test builds a10000-row,256-byte-text fixture and
warms its768-page derived cache before profiling. That first cache construction,
the retained rows/history and profiler overhead are excluded. The measured scope
exports a stable tree, checks its complete coverage again and drops the result.

The old implementation fails its128-KiB guard with7480176 requested peak bytes.
The new one observes51680 peak bytes,26894632 cumulatively allocated bytes and
142614 allocated blocks, with operation-local current bytes returning to zero.
Cumulative transient allocations are larger than the simultaneous peak because
per-page and per-row verification repeats bounded scratch. The requested peak
is not the complete retained database size, RSS or a numeric runtime heap quota.

The test preserves the relational page fingerprint and selected row location.
Separate owner/key pointer tests prove sharing; exact byte/value assertions alone
would not detect a regression to independent deep copies. The128-KiB guard applies
only to this warmed fixture/toolchain, including the second verification. A cold
cache legitimately allocates its required retained tree and needs separate limits.

## Same-config whole-model comparison

The preserved version-3 native report uses the same config as ADR0051: index-replay,
short-text keys,10000 rows, four projects,768-byte values, retained old views and
four actual scoped replay workers. Construction/staging is serial; worker replay
completes and verifies exact fingerprints, rows and row pointers before release.
Each project still has3334 history and792 index pages and unchanged root-only
physical plans with no history/index upserts.

| Implementation | Requested peak bytes | Cumulative allocated bytes |
| --- | ---: | ---: |
| Shared page ownership,4ef4a0d | 163075408 | 10046597208 |
| Shared validated primary export | 139975354 | 8945735192 |

The current report retains132919256 bytes at plans-built and133139624 at replayed;
the220368-byte difference remains map/model/worker output ownership. Final release
still observes1016 bytes/four blocks of the previously investigated coordinator/
runtime residue. This is not a claim that all process memory is zero or that every
retained row is included in a serialized image quota.

One instrumented native run takes35.74 seconds with191952-KiB maximum process RSS.
The prior shared-page observation takes35.81 seconds with205852 KiB. These elapsed
numbers include workload construction and allocation instrumentation and are not throughput
or deployment capacity claims. Source hashes bind the changed index/projection
files; the baseline report stays preserved under its published4ef4a0d source.

[The count-only report and metadata](measurements/2026-10-07-primary-export/observations.json)
contain config, counters and source hashes only. The bounded strict report fixture
test includes this fourth same-config observation. Host paths, raw stack traces,
credentials and actual user rows are excluded.

## Remaining gates

Model rebuild now carries this ownership through actual prepare, physical plan,
encoded replay and memory publication. One leaf change leaves unrelated pages
shared. Stale predecessors and obsolete pointers still refuse. Frozen EBIF/EBIX/
EBIP, managed WAL1/2, backups and durable ACKs retain their existing meanings.

Decoded plan/model/cache/transient/worker admission, hardware failure/upgrade
coverage and the combined durable writer remain open. This optimization completes
no production or stage acceptance gate.
