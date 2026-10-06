# Retained decoded physical plan admission

DecodedPlanPool reserves actual owned EBIP vector payloads before materialization.
It complements serialized EnvelopePool bytes; neither is a complete model or
process memory limit. See [ADR0053](adr/0053-admitted-decoded-image-vectors.md).

## Configuration and accounting

Explicit limits select0..4096 plan owners and0..64 MiB of vector payload. Either
zero disables new ownership. Independent pool clones use the same ledger; separate
pools have separate budgets. Atomic refusal checks owner slots first, then bytes,
and changes neither count on failure. Pending decode occupies its full reservation.

For H history images, P primary images, D retired page addresses, R changed roots
and T retired table roots, the host-layout payload is:

```text
(H + P) * size_of<PageWrite>()
+ D * size_of<PageAddress>()
+ R * size_of<RootChange>()
+ T * size_of<RetiredRoot>()
```

This includes image arrays, typed address/binding/fingerprint fields, Rust padding
and child Vec headers stored inside the root vector. Counts retain the existing
independent bounds: H256, P/D2048, R/T128 and per-root primary/retired arena1024.
The maximum host payload is a binary-local constant; it is not EBIP's9770208-byte
serialized maximum and is never persisted in a file or used as a portable ABI.

Complete borrowed preflight verifies total/digest/count/scope/order/CRC/layout
before a permit. Owned decoding then retains its existing checks/reservations.
Actual capacity of every returned vector must equal the aggregate charged shape;
unexpected extra capacity returns a typed refusal and releases private ownership.
No malformed envelope can consume a disabled pool's object/byte reservation.
No claim is made that borrowed preflight allocates zero scratch.

## Ownership and source lifetime

AdmittedPlan contains one immutable shared plan owner. Borrowing its ImagePlan
exposes only immutable existing APIs; there is no owned vector conversion. Clone
shares the same plan/image addresses and reservation without duplicating vectors.
Independent decode creates independent vectors and takes another full charge.
The last handle frees plan vectors before advertising their budget as available.
Dropping pool handles does not release still-retained owners.

Shared handles can remain usable after internal ledger poison because they allocate
no new vectors or reservation. New decode/status operations refuse; private permit
cleanup still decrements exactly once during unwind. Deliberately leaked owners
keep their charge rather than falsely releasing budget.

An admitted encoded envelope can decode through this pool while keeping its own
serialized reservation. Dropping the encoded source afterward releases only that
serialized budget. Decoded output remains independent and still needs exact replay
against its matching database/transaction/fingerprint. Wrong/stale bases refuse;
structural admission never becomes authorization or a durable acknowledgment.

Borrowed encode/replay APIs can produce new caller-owned allocations outside this
pool. The pool does not reserve raw plans from preparation, encoded output copies,
decoded models or replay outputs. The existing standalone codec remains available
for explicitly unadmitted use; these APIs are not secretly wired into runtime WAL.

## Native observation and quality gates

One isolated optional release diagnostic builds its source model/raw plan/encoded
bytes/pools before profiling. Its plan has256 history images and297 total physical
images. Reserved vector payload is1228576 bytes; requested live/peak allocation is
1228776 bytes. The200-byte owner/control allocation is outside vector accounting.
Four cloned handles leave requested bytes/blocks unchanged and retain one charge.
Last-owner drop returns operation-local requested current bytes to zero.

With new retention disabled, full valid preflight observes3292 requested transient
bytes without allocating owned image vectors. A128-KiB fixture guard rejects a
future regression to complete materialized images before quota refusal. Accepted
decode permits payload plus bounded metadata/scratch. The source/fixtures, first
model construction, profiler overhead and OS stacks are outside these measurements.
This is not a global allocator, RSS, fragmentation or cold-model quota.

Private tests force spare capacity, poison and unwind and check actual Weak ledger
release. Actual4096 independent owners fill the object cap; shared clones do not
consume new owners. Two start/retention/release-coordinated eight-thread cases hold
all successful owners while checking exact slots/bytes. An independent64-case
sequence tracks physical owner identities separately from handles and verifies
full bytes/replay for every retained plan. Maximal arithmetic permits do not claim
simultaneously allocated maximal heap workloads.

The dedicated decoded_admission sanitizer target compares the same independent
owner/handle model, corrupt-input refusal, exact budgets and complete replay.
Existing plan format, component/model capacities and old runtime recovery checks
remain active. Existing file/WAL/backup/ACK meanings stay unchanged; complete model/
cache/transient/worker reservation and production gates remain open.
