# ADR 0047: observe scoped parallel replay workers

Status: accepted for opt-in diagnostics. No server worker or byte reservation.

## Context

Earlier observations hold four replay outputs but execute reconstruction serially.
Shared append reduces their cost; overlapping worker transients still need direct
observation before choosing a numeric memory reservation.

## Decision

Add `--parallel` only to diagnostic replay/index-replay modes. Start 1..4 actual
scoped OS threads over independent borrowed base/plan pairs. Every worker reports
readiness and waits for one shared start; the coordinator releases it only after
all requested workers have arrived. OS scheduling can still serialize work.
All handles are joined, outputs remain in input order and any failure drops the
whole result group. A failed spawn/readiness wait cancels and releases already
started waiters before joining. Cleanup opens a poisoned gate while public waits
fail closed. No unbounded thread count or async database engine is introduced.

Parallel observations require version-3 count-only JSON with `parallel: true`.
The false default is omitted, preserving serial version-1/2 JSON shapes. Only
supported modes accept the flag; older decoders refuse the new version/field.
Phase/body/counter checks remain unchanged, and no stored database/WAL format
changes. The feature-gated helper is raw diagnostic model replay, outside
ModelPool admission and the normal server/CLI dependency graph.

## Verification and consequences

Five coordination cases exercise four real waiters, cancellation, poison cleanup,
ordered exact scoped outputs, a foreign group member and count refusal. Native
tests run all integer/short/long row/index modes with four actual workers, check
retained-state release and refuse unsupported flags. Version/mode properties and
preserved reports remain bounded. Actual OS resource exhaustion is not induced;
cancellation mechanics are tested independently.

Four long-key 10000-row models observe a global requested-byte peak of588108888
with row replay and575918468 with index-only replay. Instrumented Linux maximum
RSS is623588/623244 KiB. Output release leaves48 additional requested bytes in
these observations; final current bytes are1016. An attempted exact-baseline
assertion fails on that residual and is not counted as successful. Its allocation
owner is not traced; it must not be presented as proof of zero leaks or a numeric
quota. A small bounded release tolerance and verified model state are retained.

Requested counters exclude stack, fragmentation and profiler bookkeeping; peaks
include construction. Thread overlap and timing depend on OS scheduling and
instrumentation. These workloads do not prove a worst-case worker budget or
throughput improvement. Numeric transient/lifetime admission, a single durable
fence, crash/backup/upgrade and security gates remain open.

See [parallel replay observations](../parallel-replay-profiles.md).

## Subsequent owner evidence

A separate three-group, four-empty-table-scope trace on Rust1.99.0 identifies the
sole live48-byte block as the standard channel coordinator's thread-local
`std::sync::mpmc::context::Inner` created by readiness recv_timeout. All model/plan
objects are dropped and no model-state block remains in that minimal trace.
The earlier observations stay unchanged; this establishes the owner in that
shape and does not close a global leak or memory-budget gate. The raw allocation
trace stays private; the linked observations retain only count/type evidence.
