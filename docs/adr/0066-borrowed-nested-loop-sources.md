# ADR0066: borrow checked sources in the nested-loop SQL fallback

Status: accepted for the original bounded nested-loop executor.

## Reproduced problem

The fallback cloned both source tables, then cloned and concatenated rows before
ON/WHERE rejected them. A native regression on da65a95 with 100 rows per source
and 3000-byte hidden text requests 64,469,242 bytes for false ON and 64,468,894 for
false WHERE. A sorted read with 100 matches requests 64,475,392. The two-source
fixture, derived indexes and physical fingerprints precede tracking. The final
2 MiB total-request guard fails against that published code.

## Decision

Extract the existing fallback into a private synchronous nested-join executor.
Collect bounded vectors of references from the checked primary-row cursors,
which follow the same ascending primary-key order. Physical locations and full
record payloads are verified once per source before any pair is evaluated.
No source text, bytes, owned row or rejected combined row is copied.

Use the existing immutable two-source RowView for complete ON and WHERE
predicates. Charge the same pair and expression work, including both Boolean
branches. Only true ON pairs reach WHERE. Compute matched full-row bytes and
check the original cumulative byte/count bounds before making the first owned
candidate copy. Keep stable full sorting, source tie order, projection timing,
shared output accounting and unordered LIMIT stopping in their original places.
This preserves the conservative fallback limits, even when a sorted LIMIT is
small and selects narrow output. It introduces no fallback TopK or planner rule.

The global 10,000 live-row limit bounds the reference vectors. Vector capacity
and parser/planner work still allocate; this is not an allocation-free join.
Matched full payload also remains owned while retained, within its original
bound. No cloneable references escape the immutable snapshot lifetime.

The planner still exposes bounded_nested_loop for ordinary-column, OR and NOT
joins. It does not add primary point/range extraction to that plan: public left
filters still run in the original full fallback scan. Internally bound source
point/range paths remain correct for existing test plans and future explicit
planning. The obsolete private owned range-scan helper is removed; no public
Rust API or SQL/HTTP/file/WAL/cache meaning changes.

## Evidence and limits

The same native false ON/WHERE samples request 9530/9182 bytes, peaks 3960/3608,
live122 until result drop and zero afterward. The 100-match sorted sample
requests634880, peak626040, live282/drop0. Retained matched rows still account for
that larger sample. An existing 4000-row-per-source two-match comparison now
observes fallback peak80088 and primary-probe peak2622. Its obsolete requirement
that the fallback waste more than16 MiB is replaced by a256 KiB upper guard.
Historical observations remain attached to their original source versions.

The [source-bound artifact](../measurements/2026-10-07-borrowed-nested-join/operation-allocations.json)
records dimensions, actual totals/peaks, and exclusions. These figures do not
measure throughput, cold heap, RSS, allocator rounding, stacks, whole-process
quotas or retained-model admission. Primary probes still avoid pairwise work;
sharing source rows does not make a nested loop asymptotically cheaper.

Private tests compare exact pair/predicate/filter/output charges, exhausted
work, Boolean branches, public full-scan filters and internally bound sources.
Independent nullable pair models cover stable multi-key sorting, OR/NOT,
parameters, hidden UTF-8/NUL, bytes, Boolean, signed zero and repeated projection.
Real byte/count/work refusals remain, including tiny sorted LIMIT. Both WAL
versions preserve staged joins, late failure rollback, old views, restart and
verified backup restore. Bounded fuzzing compares independent cross-pair output,
NULL truth, stable source ties and explicit fallback EXPLAIN descriptions.

Numeric model/cache/staging/transient budgets, combined durable publication and
production gates remain open. The hosted allocator-runner repair is tracked
separately; no engine gate is inferred from that configuration change.
