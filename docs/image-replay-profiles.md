# Synthetic image-plan and replay allocation profiles

These opt-in measurements observe raw memory models, not a persistent database
or server. They use the same synthetic shape and ordinary diagnostic allocator
as [existing profiles](model-allocation-profiles.md). See
[ADR 0045](adr/0045-image-plan-and-replay-allocation-diagnostics.md).

```sh
cargo build --release --locked -p emilybase-model-profile --features heap-profile
target/release/emilybase-model-profile --mode replay --case long-text --rows 10000 --value-bytes 768 --projects 4 --retain-old
target/release/emilybase-model-profile --mode index-replay --case long-text --rows 10000 --value-bytes 768 --projects 4 --retain-old
target/release/emilybase-model-profile --mode replay --case short-text --rows 225 --value-bytes 768 --projects 4 --retain-old
cargo test --release --locked -p emilybase-model-profile --features heap-profile
```

`replay` replaces one row per project, builds a physical plan and independently
reconstructs the next model while the base/prepared state remain alive. Old/new
values, exact fingerprints and physical boundary row locations are checked before
publication. `index-replay` leaves relational history and image bodies unchanged,
but creates and verifies a successor root selection. Zero image-body bytes do
not mean an empty transaction: root metadata still changes.

Version-2 count-only JSON adds `images_per_project` and four explicit phase
samples for plan/replay retention and release. Existing version-1 modes/reports
keep their previous shape and remain accepted. New modes require version two;
older decoders refuse them. No stored database/WAL format changes. The decoder
limits reports to 8192 bytes, rejects unknown fields, invalid phase/version/mode,
inconsistent counts/arithmetic and regressed counters. A valid unsigned document
does not authenticate a measurement or authorize a memory quota.

## Local observations, 2026-10-06

Linux, stable Rust 1.99.0, optimized release, pinned dhat 0.3.3. All three actual
commands exit zero and verify their synthetic states; reports preserve actual
counter values. All projects' plans and replayed outputs coexist. Operations are
serial; no overlapping concurrent worker transient is measured.

| Requested-byte sample | Four long-key 10000-row models, row replay | Same shape, index-only replay | Four short-key 225-row models, row replay |
| --- | ---: | ---: | ---: |
| Built | 574193688 | 574193688 | 3344848 |
| Prepared | 581136352 | 574523096 | 4153272 |
| Plans built | 581156704 | 574526904 | 4190168 |
| Replayed outputs held | 1154403176 | 574855448 | 7216216 |
| Replayed outputs released | 581156704 | 574526904 | 4190168 |
| Plans released | 581136544 | 574523288 | 4153464 |
| Published with old views | 581135680 | 574522424 | 4152600 |
| Old views released | 574210960 | 573874904 | 3344296 |
| Global requested-byte peak | 1154413224 | 575918468 | 7588264 |
| Final requested bytes | 968 | 968 | 968 |
| Instrumented maximum RSS, KiB | 1242644 | 623380 | 12532 |
| Instrumented elapsed | 5.62 s | 4.03 s | 0.43 s |

Row replay of four long-key models carries just 16384 original image-body bytes,
but adds 573246472 requested live bytes at the replayed sample. The current
implementation clones/replays the complete bounded history and materializes
separate rows/maps. Index-only replay shares immutable relational objects and
adds 328544 bytes. Releasing outputs restores the exact plans-built live sample
in all three shapes. Plans themselves and per-project report metadata also have
allocation costs beyond their image bodies.

Requested bytes exclude profiler bookkeeping, stack and fragmentation. RSS is a
separate process metric and includes instrumentation. Peak counters are cumulative
from construction: subtracting two peaks cannot derive an operation-local peak.
The remaining final bytes are diagnostic/report collections; database-state
objects have been dropped. Timings include instrumentation and are not throughput
comparisons. These shapes do not prove worst-case, leak, concurrent-worker or
server-reservation bounds.

Preserved reports: [long row replay](measurements/2026-10-06-image-replay/replay-long-four.json),
[long index replay](measurements/2026-10-06-image-replay/index-replay-long-four.json),
[short row replay](measurements/2026-10-06-image-replay/replay-short-four.json).
Byte/transient admission, optimized history replay, shared durable fences,
crash/backup/upgrade and security gates remain open.

The subsequent [shared append observations](shared-history-replay.md) repeat the
same row-replay shapes while retaining unchanged immutable pages/bodies. Earlier
reports above remain historical; numeric transient and durable gates stay open.
