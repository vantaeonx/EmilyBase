# Native snapshot coordination under original object owners

`NativeFileRoot::with_snapshot(work)` runs trusted synchronous readonly coordination
while holding every actual original physical object descriptor, including charged
orphans, plus the original metadata/object owners and common-root selections.
The callback receives the existing immutable sensitive `FileSnapshot`. Its value
is returned only after final original-source proofs. No descriptor, mutable store
or current user/file authority is supplied to the callback.

The earlier `capture()` keeps its existing single-capture contract and bytes. Its
result owns copied data, so original object descriptors end with capture. Merely
retaining that result during later work does not retain those source files. An
executed prototype captures, replaces an object with identical bytes during later
work, then checks a fresh inventory. Its expectation of original-file refusal fails:
the longer lifetime was never established. This is a counterexample to that
composition, not a defect in the documented single-capture API.

## Checked lifetime

Admission validates the original common root, metadata history and complete object
graph. Existing capture acquires descriptors for every physical object before the
first metadata prefix, validates/copied metadata and objects, and verifies the
original inventory/WAL. Narrow field borrows then check common selection immediately
around callback work while the store still holds those original payload descriptors.

After work, every retained original object is fully verified again, including
selected inode, private mode, single link, stamp and complete bytes. Inventory and
acknowledged WAL must still equal the admitted source. Final common-root state must
also match. Failure withholds/drops the returned value, preserves the actual source
entries and poisons the root until explicit reopen. No source is repaired/adopted.
Ordinary capture uses its original branch, without the new callback's extra final
verification pass.

Callbacks have type `FnOnce(FileSnapshot) -> T`; the outer result describes source
verification. A caller may choose `T = Result<U, E>` for its own work error. A healthy
source returns `Ok(Err(e))` in that case and remains usable. Nested coordination must
propagate both result layers before publishing any joint output. Actual source
failure still poisons the changed source. Native callbacks must defer external
publication/effects until the complete outer result succeeds; this API cannot undo
arbitrary caller effects.

Nested callbacks can retain the first component's actual object readers while
copying the next component. A later replacement in either component prevents a joint
image result; an unchanged outer source is not poisoned by an inner error value.
Copied snapshots returned after success retain no payload descriptors and preserve
the existing paired archive/fresh restore behavior. Normal absolute and retained
parent-relative selection rules remain unchanged, including moves/refusal.

## Bounds and remaining gates

Existing per-store object/count/byte bounds and allocation failures remain. Retaining
all readers and repeated full verification add descriptor, hash and scratch costs.
Caller-controlled nesting/retained copies do not have an aggregate FD/heap/worker
reservation. A later account/file collector must define and admit its global budget
before opening every original object. Native callers/ancestor configuration remain
trusted; checks are finite observations, not filesystem leases or a hostile-host
sandbox. Callback panics and arbitrary caller side effects are outside the contract.

This is a functional native coordination boundary. It supplies no common account/
file runtime roster, joined archive encoder/restore, user download/upload permission
or HTTP endpoint. Existing file/schema/archive/credential versions and durability
order remain unchanged. See [ADR0153](adr/0153-retained-native-file-snapshot-coordination.md).

## Executed evidence

[Source-bound checks](measurements/2026-10-15-native-file-root-coordination/verification.json)
record685 affected cases per stable1.99/minimum1.89:315 native storage/object/files,
45 actual CLI and325 original engine/WAL/transactions/backup cases. All builds,
formatting, strict workspace/fuzz lint and minimum fuzz compilation exit0.637 frozen
source hashes match after every process closes. Prior52b84b6 separately passed the
complete hosted1740/0/43 workspace per toolchain and all five jobs, with636 committed
source hashes checked. It is not a new complete-workspace result for this source.

Twelve new cases include32 generated independent payload/revision/orphan/fresh-copy
models, actual descriptor lifetime/return, nested distinct projects, callback error
values and failed-value dropping. Eight same-byte object replacements,32 corruption/
mode/link/missing changes and30 namespace/private-parent changes refuse. Ten cases
span empty/1/8192/8193/8MiB readonly payloads and exact archive compatibility. Four
parent moves inside work preserve relative selection and normal absolute refusal.
Sixteen actual signal9 kills verify original data/object locks and actual referenced/
orphan FD presence during work versus absence after success, both WAL versions,
normal/relative mode and empty/8193-byte payloads. Exact source WAL/reopen survives;
existing227 file-related kills rerun, for243 per toolchain. Initial generic/lint/test
fixture failures and the composition counterexample are recorded separately.
