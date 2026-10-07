# ADR 0055: discard caller spare capacity before retaining records

Status: accepted for live relational payload ownership. File formats unchanged.

## Reproduced problem

Owned Rust inputs can have small valid values but large unused String/Vec capacity.
State::apply retained the caller's schema/row allocation directly. A five-byte
schema name retained 131072 bytes; a three-value row retained 1024 element slots.
The dedicated native insertion fixture retained 2132013 requested bytes after
storing only a small integer, three-byte UTF-8/NUL text and three byte values.
Both public regression tests and the native guard failed before implementation.

Serialized HTTP/SQL/file values carry lengths rather than Rust capacities, but the
owned engine API must still establish a predictable retained shape before numeric
payload admission. Logical/encoded length caps alone do not constrain caller spare
capacity. Recovery already reconstructs payload from bounded encoded lengths.

## Decision

After complete existing logical validation and before retaining live state,
State::apply consumes the incoming Event through private payload compaction.
Compact schema/table/column names, the column vector, inserted/replaced row vectors
and every owned text/byte payload. Text delete keys use the same helper; fixed
scalar/root/drop variants preserve their representation and event meaning.

Exact-capacity inputs use a fast path retaining the same actual buffers. Oversized
capacity passes through Box<str>/Box<[T]> ownership and back to String/Vec. Stored
record vector types have nonzero element size; the returned shape exposes no spare
elements. No borrowed old row/schema/key is mutated. There is no new public API,
unsafe block, dependency, format field, allocator or implicit deployment limit.

Keep validation/refusal order, typed errors, primary-key rules and existing wire
encoding. This applies to the shared State used by direct files, snapshots,
transactions, replay and the optional model. Invalid events still leave previous
state untouched. Old immutable snapshots retain their unchanged allocations.

## Evidence and limits

Five private cases preserve exact ETBL bytes, all value kinds, negative-zero bits,
maximum Unicode text deletes, empty payloads and actual buffer identity through
repeated exact-capacity compaction. Seven public cases cover original regressions,
insert/replace, physical history/location parity, old readers, atomic refusal,
direct file reopen, 64 columns and 3072-byte text keys. A 48-case independent
history model compares complete rows and exact canonical page fingerprints.
Two managed cases execute commit/recovery/checkpoint/compaction and rollback/abort
in WAL 1/2 while preserving old readers and exact rejected WAL bytes.

The same isolated native insertion fixture now observes 2195 requested retained
bytes and 2130282 peak bytes, versus 2132013 retained/2132237 peak before the fix.
Input construction is included in those peaks; the empty fixture precedes profiling.
After snapshot release operation-local current requested bytes return to zero.
This is a retained-shape regression, not a bound on caller input or whole-process
heap. Allocator rounding/fragmentation, stacks and profiler bookkeeping are excluded.

Caller input can already own arbitrary allocations before invoking the engine.
Compaction may temporarily coexist with it and does not prevent that allocation.
Box conversion establishes reported payload capacity, not allocator usable-size or
RSS guarantees. Page/cache/map/node/Arc overhead and model/staging/replay transients
still require separate accounting. Complete durable-index and production gates
under ADR 0031 remain open. No data migration or runtime ACK change is selected.
