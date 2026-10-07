# Retained record payload capacity

The live relational engine now discards unused capacity from owned incoming schema
and row payloads after validation, before retaining them. See
[ADR 0055](adr/0055-compact-retained-record-payloads.md).

## Values and wire bytes

Names, column vectors, row vectors, text strings and byte vectors retain exactly
their nonzero-sized record element/byte lengths. Existing exact-capacity buffers
remain in place. Conversion through owned boxed strings/slices removes spare
capacity without changing scalar values, UTF-8/NUL bytes, signed integers,
negative-zero float bits, nullability, key rules, column counts or ETBL encoding.

The original source of a changed event is consumed; old immutable rows, table maps,
keys and locations are never compacted in place. Existing validation runs first.
Invalid events preserve prior data/file bytes, and rolled-back or aborted managed
transactions preserve their whole original WAL. Stored record formats and frozen
WAL 1/2 compatibility retain their previous meaning.

This shared state boundary serves the direct file API, Snapshot, managed
transactions and optional staged/replayed models. There is no new public allocator
or memory configuration. Recovered rows already derive their capacities from
bounded physical record lengths; this aligns accepted owned inputs with that shape.

## Reproduction and native observation

Before the fix, public regressions observe a five-byte schema name with 131072
reserved bytes and a three-value row with 1024 reserved elements. Both fail their
retained-capacity assertions. A separate release-native fixture constructs two
1-MiB text/byte reservations and a 1024-slot row inside the profiler, then inserts
only a small integer, three bytes of UTF-8/NUL text and three byte values.

Before the fix that operation retains 2132013 requested bytes, peak 2132237. After
compaction it retains 2195, peak 2130282; after snapshot release its current bytes
return to zero. The peak includes the deliberately large incoming allocations.
The empty fixture precedes profiling; stacks, profiler and allocator overhead are
excluded. The 64-KiB retained guard detects this exact regression, not all possible
valid shapes, cold cache construction or model/transient heap costs.

The caller may allocate large input before invoking the engine, and temporary
compaction can coexist with that input. Reported Vec/String capacity does not
bound allocator usable-size, fragmentation or RSS. Numeric model/cache/staging/
replay-transient and runtime worker budgets remain open. This change enables a
predictable retained payload shape; it does not declare production readiness.
