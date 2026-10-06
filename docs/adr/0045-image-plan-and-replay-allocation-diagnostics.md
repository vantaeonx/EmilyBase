# ADR 0045: observe image-plan and replay allocations

Status: accepted for opt-in diagnostics. No runtime memory reservation or writer.

## Context

The physical image bound in ADR 0044 excludes independent replay outputs and
temporary full-history reconstruction. A count-based generation reservation
cannot charge those bytes; a small image plan may reconstruct a large state.
Existing state/fingerprint profiles stop short of that operation.

## Decision

Extend the existing synthetic allocation binary with `replay` and `index-replay`.
Use the same 1..10000-row, 1..4-project, integer/256-byte/3072-byte-key workloads
and original model/index engines. One row changes in replay mode; index-replay
publishes only a successor root over unchanged pages. Optional old views remain
held. Prepared states, all owned plans and all independently replayed states are
held simultaneously; construction, replay and publication run serially.

Sample requested allocations at built, staged, indexes-staged, prepared,
plans-built, replayed, replay-released, plans-released, published,
old-views-released and released. Verify exact base/next fingerprints, row counts,
old/new boundary values and physical row addresses before publication/release.
Full model replay independently validates all selected root/row coverage.

Only these new modes emit version-2 diagnostic JSON, with exact per-project
history/primary upserts, retired IDs, changed/retired roots and image-body bytes.
The 8192-byte decoder enforces workload-specific count/phase/version consistency,
checked body arithmetic and counter monotonicity. Version-1 modes retain their
previous JSON shape; historical reports still pass. Older decoders refuse the
new mode/version. This is not a database, WAL or network format change. Reports
remain unsigned counts with no data, host paths, credentials or backtraces.

## Consequences and evidence

Nine real-process cases and sixteen report cases pass in the opt-in release
suite, including retained-state cleanup, damaged counts/mode/version, unknown
nested fields, every report truncation and 128-case mixed-mode codec properties.

Four held 10000-row long-key models with 768-byte values produce only four
4096-byte history images and no primary images. The replayed current sample adds
573246472 requested bytes over plans-built because complete histories/live rows
are reconstructed. Release returns that sample exactly to plans-built. The
requested global peak is 1154413224 bytes; instrumented Linux maximum RSS is
1242644 KiB. Index-only replay of the same shape adds 328544 requested bytes,
returns them on release and peaks at 575918468 requested bytes. These are observed
costs, not a universal bound, operation-local peak or enforceable reservation.

The final 968 requested bytes are diagnostic/report collections still alive at
the final sample. The allocator's own bookkeeping, stack and fragmentation are
excluded; Linux RSS includes instrumentation. Four outputs coexist, but this
does not measure overlapping concurrent replay worker transients. No new WAL,
durability acknowledgment, real data or server integration follows. Numeric
byte/transient admission and reduced history-replay copying remain required.

See [reproduction and preserved observations](../image-replay-profiles.md).
