# Verification

Run `cargo test --locked --workspace` for unit, file integration, subprocess CLI
and property tests. Each property runs 256 generated cases by default. File tests
use isolated temporary directories and synthetic payloads. They exercise reopen,
no-clobber creation, concurrent creation, advisory locks, truncation, corruption
and record mutation. They do not prove power-loss safety or transactional recovery.

## Format fuzzing

The engine builds on stable. Optional coverage-guided fuzzing uses nightly and
`cargo-fuzz` with AddressSanitizer:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
cargo +nightly fuzz run file_format -- -max_total_time=30 -max_len=4096 -rss_limit_mb=512
```

The target checks raw headers, pages and pages with a repaired checksum to reach
structural validation. Add valid synthetic header/page seeds to
`fuzz/corpus/file_format` for better coverage. Corpus and crash artifacts are
ignored. A bounded smoke run is not a complete fuzz campaign or a security audit.

## Pending acceptance tests

Forced termination during WAL and page writes, committed versus uncommitted
recovery, torn-write repair, checkpoint crashes, database-level concurrency,
backup/restore and cross-project authorization require their implementations.
None of those acceptance gates is satisfied by the current tests.

## First increment: executed checks

On 2026-10-02, on Linux with Rust stable 1.99.0:

- Formatting, Clippy with warnings denied, workspace build and all 21 tests passed.
- Three properties ran 256 cases each, including random operation sequences.
- The final AddressSanitizer format-fuzz smoke run completed 1,332,572 executions
  in 16 seconds without a crash (configured time budget: 15 seconds).
- Source size: 999 physical Rust lines, including tests and the fuzz target;
  907 lines excluding blank lines and comment-only lines. Documentation,
  manifests, lockfiles and generated build output are excluded.

These observations apply to this increment, not later revisions or the pending
transaction, recovery and security acceptance gates.
