# Synthetic model allocation profiles

This opt-in tool measures the original in-memory prototype. It neither writes a
database nor enables the proposed durable index writer. Use synthetic data only.
See [ADR 0040](adr/0040-opt-in-model-allocation-diagnostics.md).

## Reproduce

From the repository root, on Linux with the declared Rust floor or current stable:

```sh
cargo build --release --locked -p emilybase-model-profile --features heap-profile
target/release/emilybase-model-profile --help
target/release/emilybase-model-profile --mode fingerprint --case short-text --rows 10000
target/release/emilybase-model-profile --case short-text --rows 10000 --value-bytes 768 --retain-old
target/release/emilybase-model-profile --case long-text --rows 10000 --value-bytes 768 --retain-old
target/release/emilybase-model-profile --case long-text --rows 10000 --value-bytes 768 --projects 4 --retain-old
cargo test --release --locked -p emilybase-model-profile --features heap-profile
target/release/emilybase-model-profile --mode index-only --case long-text --rows 10000 --value-bytes 768 --projects 4 --retain-old
```

The binary emits one version-1 JSON report to stdout. Shell redirection can retain
that report; the profiler itself does not create allocation trace files. Inputs
are limited to 1..10000 rows, 1..4 projects and 0..768 value bytes. Fingerprint mode
supports integer and 256-byte text keys and refuses retained-view/long-key options.
No passwords, API keys, row values, paths or backtraces appear in reports. There
is no CLI argument for loading a real project, external data or server credentials.

`state` creates a two-column table and fills it in batches of at most 256 events.
It captures built state, relational cloning, row/index staging, preparation,
publication, old-view release and final model release. One row is replaced in
each project; old/new values and total rows must agree before success. Table
history accumulates throughout construction and is included in the observation.

`--projects 4` holds four independently scoped models and all four private stages
at once. Their construction and publication are serial. It does **not** execute
four simultaneous workers or measure their overlapping temporary peaks. It also
does not model arbitrarily many retained reader generations.

## Counter meanings

The pinned [dhat HeapStats API](https://docs.rs/dhat/latest/dhat/struct.HeapStats.html)
counts requested live bytes/blocks, cumulative allocated bytes/blocks, and a
global byte peak. `peak_blocks` is the number of blocks **at that byte peak**;
it need not increase monotonically and is not the maximum observed block count.
The peak is cumulative from profiler start and cannot be subtracted to produce a
phase-local peak. The [builder testing mode](https://docs.rs/dhat/latest/dhat/struct.ProfilerBuilder.html)
suppresses automatic output. The allocator is linked only into the feature-gated
binary; report decoding and the runtime server use their usual allocators.

Counters include model construction and small diagnostic/report collections.
They do not include the profiler's own internal bookkeeping, stack memory,
allocator fragmentation or all process mappings. Report serialization runs after
the profiler is dropped. The process maximum RSS, measured separately with Linux
`/usr/bin/time -v`, includes instrumentation and is a different quantity.

Component counts are exact encoded standalone objects: history page images,
one EBIF header per index and one 192-byte root. They neither equal heap bytes
nor describe a future WAL envelope. The bounded decoder verifies consistency;
an unsigned report is not evidence that a third party really ran the workload.

## Observed local release runs, 2026-10-05

Rust 1.99.0 stable, default optimized release profile, Linux, dhat 0.3.3. Every
run exits successfully and verifies its synthetic state. Reports below preserve
the actual counters; the preserved files also pass the bounded Rust decoder.
The timings include instrumentation and are not comparative throughput results.

| Workload | Built requested bytes | Peak requested bytes | Published with old view | After old-view release | Final requested bytes | Maximum process RSS, KiB | Elapsed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 10000 short keys, fingerprint comparison | 3283048 | 10785752 | n/a | n/a | 488 | 17452 | 0.08 s |
| 10000 short keys, 768-byte values, one model | 36105412 | 68678052 | 61165396 | 36079490 | 608 | 91924 | 137.11 s |
| 10000 long keys, 768-byte values, one model | 143229196 | 246327380 | 246321182 | 143233466 | 608 | 255036 | 2.27 s |
| Same long-key shape, four held models | 572915440 | 985295630 | 985283384 | 572932520 | 608 | 1008592 | 9.79 s |

The four-model run peaks near 940 MiB of requested allocations. The old-view
release lowers current requested bytes by 412350864. This is a concrete retention
cost in this shape, not the system's worst-case heap or an acceptable server quota.
Remaining few hundred bytes are diagnostic/report collections still alive at
the final sample, not a database-state leak.

The fingerprint tree has 768 pages and a 3149824-byte EBIF. Full encoding requests
21805352 bytes in 21056 allocation calls; streaming requests 18651432 bytes in
21054 calls. The difference is 3153920 bytes and two calls; both hashes match.
The streaming path still allocates image vectors and reconstructed topology.
The shared cumulative peak includes the full-encoding baseline, so this run proves
lower allocation traffic, not a separately measured streaming peak.

The short-key state ends with 3334 history pages, 792 selected index pages and
16904384 encoded component bytes. Each long-key state has 10001 history pages,
one empty index page and 40972480 encoded component bytes. These selected states
include one replace and are distinct from the separately tested dense rebuild.

Preserved reports: [fingerprint](measurements/2026-10-05-model-profile/fingerprint-short.json),
[short state](measurements/2026-10-05-model-profile/state-short.json),
[long state](measurements/2026-10-05-model-profile/state-long.json),
[four long states](measurements/2026-10-05-model-profile/state-long-four.json).

## Remaining admission work

Address state cloning and retained-reader lifetimes before selecting a numeric
reservation. Measure concurrent transient stages, replay and long history,
fragmented/mixed tables, compaction and backup on representative hardware. Then
test a real rejection/release mechanism independently of these measurements.
The optional [lifetime coordinator](model-lifetimes.md) now tests count-based
reservation/release for retained/pending generations, readers and writers. It does
not assign measured-byte costs or claim a heap/server reservation.
Encoded-image admission, the four HTTP worker permits and diagnostic row bounds
do not enforce a common heap quota. Shared durable WAL records/replay and broader
power-loss, security, backup/upgrade and load acceptance remain open.

## Shared-table follow-up

[ADR 0041](adr/0041-shared-relational-snapshot-tables.md) changes the runtime
Snapshot's private table/location-map ownership to per-table copy-on-write.
Previous report modes/shapes remain accepted; `index-only` is an additional
mode with the same state phase order/components and no row replacement. It
publishes a new complete index/root memory selection while verifying unchanged
rows and physical history. It does not make an independently durable index.

Two later local release runs use the same Linux/Rust/dhat settings, four held
models, 10000 long keys and 768-byte values each:

| Sample | Earlier eager begin, row-write mode | Shared-table row-write mode | Shared-table index-only mode |
| --- | ---: | ---: | ---: |
| Built requested bytes | 572915440 | 572912112 | 572912112 |
| At begin, requested bytes | 984941760 | 573235312 | 573235312 |
| Indexes staged, requested bytes | 985292312 | 985285656 | 573249168 |
| Published retaining old view, bytes | 985283384 | 985276728 | 573240656 |
| After old-view release, bytes | 572932520 | 572929192 | 572593136 |
| Global requested-byte peak | 985295630 | 985288974 | 675834188 |
| Instrumented maximum process RSS, KiB | 1008592 | 1008768 | 688696 |
| Instrumented elapsed | 9.79 s | 9.31 s | 9.42 s |

Begin avoids the former whole-state row copy: its current-byte sample is lower
by 411706448 bytes in this shape. Index-only staging then keeps shared relational
objects; its old-view release drops just 647520 bytes of metadata/handles/old
index selections. Its cumulative peak still includes construction, not just
index-only publication. Actual row-write mode still detaches each large table
and per-table location map; its approximately 940-MiB peak remains. Neither the
timing difference nor RSS establishes throughput or a server reservation.

Actual later reports: [shared row writes](measurements/2026-10-05-shared-tables/state-long-four.json)
and [shared index-only stages](measurements/2026-10-05-shared-tables/index-only-four.json).
The preserved earlier reports remain historical observations and are not replaced.

## Shared row-body/key follow-up

[ADR 0042](adr/0042-shared-row-bodies-and-live-keys.md) shares immutable keys and
row bodies when a table/location map detaches. A later run uses the same optimized
Linux/Rust/dhat configuration and four long-key 10000-row models, 768-byte values,
one actual replacement per model and retained old views.

| Sample | Table sharing only | Shared bodies/keys inside detached maps |
| --- | ---: | ---: |
| Built requested bytes | 572912112 | 574193520 |
| At begin, requested bytes | 573235312 | 574516720 |
| Indexes staged, requested bytes | 985285656 | 581144248 |
| Published with old views, bytes | 985276728 | 581135320 |
| Old views released, bytes | 572929192 | 574210600 |
| Global requested-byte peak | 985288974 | 581147606 |
| Maximum instrumented process RSS, KiB | 1008768 | 623532 |
| Instrumented elapsed | 9.31 s | 3.88 s |

The requested peak falls by 404141368 bytes, about 385 MiB. Baseline grows by
1281408 bytes in this shape because ownership representation/allocation costs
change. These are measured tradeoffs, not a universal upper bound or throughput
claim. Retained generations still own map structures and changed rows; no
reservation policy is enabled. [The actual report](measurements/2026-10-05-shared-rows/state-long-four.json)
passes the same bounded decoder alongside earlier historical reports.
