# Synthetic parallel memory replay

The opt-in diagnostic can now launch up to four real scoped replay workers with
a shared start and retain all their outputs. This extends the earlier
[serial shared append observations](shared-history-replay.md); it enables neither
server workers nor durable database writes. See
[ADR 0047](adr/0047-scoped-parallel-replay-observations.md).

```sh
cargo build --release --locked -p emilybase-model-profile --features heap-profile
target/release/emilybase-model-profile --mode replay --parallel --case long-text --rows 10000 --value-bytes 768 --projects 4 --retain-old
target/release/emilybase-model-profile --mode index-replay --parallel --case long-text --rows 10000 --value-bytes 768 --projects 4 --retain-old
cargo test --release --locked -p emilybase-model-profile --features heap-profile
```

Each requested worker reports readiness before waiting on a shared start. All
threads are joined; result order remains the project input order. A wrong base
in any member refuses the whole group and preserves every base. Failed thread
creation/readiness cancels and releases already-started waiters. The OS determines
the actual execution overlap; a shared release does not guarantee simultaneous
instructions. Projects are still constructed and published serially.

Only replay/index-replay accept `--parallel`. The report uses version three and
`config.parallel: true`; existing version-one/two modes omit the false default
and keep their JSON shapes. The bounded8192-byte decoder verifies flag/mode/version,
phases, per-project components and counters. Reports contain no user data, host
paths, thread identities, secrets or backtraces. They remain unsigned observations.

## Actual local observations, 2026-10-06

Same Linux, stable Rust1.99.0, optimized release and pinned dhat0.3.3 settings.
Four independent10000-row long-key models,768-byte values, retained old views.
Both actual commands exit zero and verify exact fingerprints, old/new boundary
values and physical row locations. Counter values below are preserved.

| Requested-byte sample | Parallel row replay | Parallel index-only replay |
| --- | ---: | ---: |
| Plans built | 581156704 | 574526904 |
| Outputs held | 588098552 | 574855496 |
| Outputs released | 581156752 | 574526952 |
| Published with old views | 581135728 | 574522472 |
| Old views released | 574211008 | 573874952 |
| Global requested-byte peak | 588108888 | 575918468 |
| Final requested bytes | 1016 | 1016 |
| Instrumented maximum RSS, KiB | 623588 | 623244 |
| Instrumented elapsed | 4.35 s | 3.98 s |

The observed row replay peak is336 requested bytes above the earlier serial
shared-append peak in this shape. It is not a worst-case concurrent bound. An
additional48 current bytes remain after output release, unlike exact-baseline
serial release. An exact-equality test failed on that residual; the final verified
checks require bounded cleanup and correct retained state. The residual allocation
was not traced, so this is not evidence of zero leaks. Final current bytes also
include diagnostic/report collections still alive at the sample.

Peaks include construction/verification, requested bytes exclude profiler overhead,
stack and fragmentation, and instrumented RSS is a separate measure. Timings do
not establish throughput or speedup. Full process/transient/worker byte reservation
and complete durable/crash/security gates remain open. No real project is loaded.

Reports: [parallel row replay](measurements/2026-10-06-parallel-replay/replay-long-four.json),
[parallel index replay](measurements/2026-10-06-parallel-replay/index-replay-long-four.json).

## Residual owner follow-up

A separate minimal synthetic program on the same Rust1.99.0 uses pinned dhat with
allocation backtraces. It builds four empty-table physical plans, invokes the same
parallel helper three times, drops each group, then drops all plans and bases.
Current requested bytes increase by48 after the groups; only48 bytes in one block
remain before process exit. The live allocation's stack identifies
`std::sync::mpmc::context::Inner`, allocated by the coordinator's readiness
`recv_timeout` through the standard channel's thread-local Context. No model-state
allocation remains in this minimal trace. Repeating the group does not grow that
residue here. This is one traced shape, not a general leak or memory-quota proof.

The earlier reports remain unchanged. The raw trace contains local paths/process
identity and stays outside the public repository. Only the checked
[count/owner summary](measurements/2026-10-06-parallel-replay/residue-owner.json)
is published. The previous untraced/exact-equality failure describes the earlier
checkpoint, and this follow-up supplies its owner evidence. Byte/transient,
broader workload and durability gates remain open.
