# ADR0064: check bounded schema names without a temporary heap set

Status: accepted for original catalog validation. All schema rules remain active.

## Reproduced problem

Schema/key validation allocated a temporary ordered set on every call, including
physical row resolution. Against46317ba,1000 paired schema/key checks request
208000 bytes for one/two columns and2272000 for64 columns. A native zero-temporary-
allocation regression fails before implementation. The preceding physical-row
repair still requested104000 metadata bytes per1000 resolutions.

## Decision

After the original table-name and1..64 column bounds, store borrowed column names
and original positions in fixed stack arrays. Sort at most64 bounded names by
name then position and flag subsequent occurrences. Revisit columns in original
order, checking identifiers before duplicate flags, then original primary rules.
Thus a duplicate before a later invalid identifier still wins, while an earlier
invalid identifier still wins over later duplication. Type/nullability/primary
and row/key/value checks are not skipped or cached away.

Oversized names use empty placeholders while preparing sort input, so no string
longer than63 bytes enters comparisons before its typed refusal. A placeholder
cannot collide with any earlier valid identifier. The original identifier error
is emitted at its original position. Immutable caller schemas are never changed.
Fixed array payload is bounded by MAX_COLUMNS; in-place sorting introduces no
owned names or growing collection. This contains no unsafe and changes no public
API, schema grammar, serialized format, file/cache/WAL version or durable ACK.

## Evidence and limits

The same native one/two/64-column samples request zero temporary heap bytes/blocks,
peak/live zero. Both1000-resolution physical samples also request zero after the
metadata change. Fixtures/names/keys/locations precede profiling. Exact hashes,
dimensions and excluded allocator rounding/stacks/profiler data are in the
[source-bound artifact](../measurements/2026-10-07-stack-schema-validation/operation-allocations.json).
This is an allocation observation, not throughput, cold memory, RSS, stack quota
or whole-process budget. The arrays replace a small heap set with bounded stack
work; all other query/model/cache/staging/transient costs remain separate.

Tests compare original error precedence, all2016 distinct duplicate-position
pairs at64 columns, maximum63-byte names, count65 refusal, oversized later names
and independent sequential-set models. Bounded fuzzing generates invalid table/
column names, duplicates, nullable/type/primary boundaries and preserves input.
Full original SQL/format/recovery suites remain required. No production gate closes.
