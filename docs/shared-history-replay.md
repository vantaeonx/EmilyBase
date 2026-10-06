# Shared immutable history during memory replay

The original synchronous Snapshot can replay a bounded physical append over a
validated base without rebuilding its entire history. The experimental ImagePlan
uses this path after checksum, identity, exact base and typed address validation;
complete tree/row coverage and the expected next state hash remain mandatory.
This writes no files, WAL or durable acknowledgment. See
[ADR 0046](adr/0046-shared-tail-history-replay.md).

```rust
use emilybase_database::Snapshot;
use emilybase_storage::Page;

fn replay(base: &Snapshot, pages: &[Page]) -> emilybase_database::Result<Snapshot> {
    // pages must originate from checksum-validated decoding or original Page
    // operations. Ownership/fences belong to the surrounding model, not Page.
    base.replay_append_pages(pages)
}
```

An empty append returns a handle-sharing clone. Nonempty input admits at most
256 changed pages and 256 new records. It starts at the last base page or its
successor, then proceeds contiguously. A last-page extension preserves all prior
slot bytes. Deleted/empty records, duplicate/order/gap errors, earlier rewrites,
invalid catalog operations and global row/table/history limits refuse. Later
errors discard all earlier private changes. The supplied base's history and
logical data remain unchanged.

New slots pass through the original event application. Physical images and final
page count must then agree with the original canonical writer. Even valid events
cannot choose a different page packing. Unchanged tables/row bodies/keys and prior
pages remain shared; affected map structures and changed rows detach. The method
does not authorize a foreign database, bind a transaction or select a durable
state. Callers needing those guarantees use the surrounding exact ImagePlan.

Full recovery from unknown disk history still uses its existing complete decoder.
This append optimization does not change stored EBPG/WAL formats or enable the
proposed combined writer. Raw outputs remain outside ModelPool admission.

## Observed repeat measurements, 2026-10-06

Same Linux, stable Rust 1.99.0, optimized release and pinned dhat configuration
as [the earlier image replay profiles](image-replay-profiles.md). Four project
plans and their outputs coexist; operations are serial, not overlapping workers.
Both commands exit zero and verify old/new values, hashes and current locations.

| Requested-byte sample | Earlier four long-key 10000-row models | Shared append, same shape | Earlier four short-key 225-row models | Shared append, same shape |
| --- | ---: | ---: | ---: | ---: |
| Plans built | 581156704 | 581156704 | 4190168 | 4190168 |
| Replayed outputs held | 1154403176 | 588098504 | 7216216 | 4997728 |
| Additional current bytes | 573246472 | 6941800 | 3026048 | 807560 |
| Outputs released | 581156704 | 581156704 | 4190168 | 4190168 |
| Global requested-byte peak | 1154413224 | 588108552 | 7588264 | 5347056 |
| Final requested bytes | 968 | 968 | 968 | 968 |
| Instrumented maximum RSS, KiB | 1242644 | 623460 | 12532 | 10436 |
| Instrumented elapsed | 5.62 s | 4.06 s | 0.43 s | 0.39 s |

The long-key replay live addition falls by 566304672 requested bytes, about
540 MiB, while the original changed image-body count remains 16384 bytes. Output
release restores the exact plans-built sample in both repeated shapes. This
observes sharing; it does not enforce a process-wide heap limit or measure every
history shape. Peaks include construction and verification, not a phase-local
maximum. RSS includes instrumentation; requested bytes exclude bookkeeping,
fragmentation and stack. Timings are not throughput claims. Final few hundred
bytes are diagnostic/report collections, not retained database states.

New observations: [long keys](measurements/2026-10-06-shared-replay/replay-long-four.json)
and [short keys](measurements/2026-10-06-shared-replay/replay-short-four.json).
Historical reports are preserved. Numeric transient/replay/worker admission,
selected WAL encoding, one durable fence, crash/backup/upgrade and security gates
remain open.
