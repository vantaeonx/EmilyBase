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
cargo +nightly fuzz run catalog_records -- -max_total_time=30 -max_len=4096 -rss_limit_mb=512
```

The target checks raw headers, pages and pages with a repaired checksum to reach
structural validation. Add valid synthetic header/page seeds to
`fuzz/corpus/file_format` for better coverage. Corpus and crash artifacts are
ignored. A bounded smoke run is not a complete fuzz campaign or a security audit.
The catalog target checks schema, row and relational-event codecs and round trips
accepted records. Synthetic `ESCH`, `EROW` and `ETBL` seeds improve its coverage.

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

## Table increment: executed checks

On 2026-10-03, on Linux with Rust stable 1.99.0:

- Workspace and fuzz-target formatting, Clippy with warnings denied, workspace
  build and all 58 tests passed.
- Eight CPU properties run 256 cases each; the persistent CRUD model property
  runs 32 cases and reopens the file after each generated operation.
- Schema, primary-key, row/type/nullability and global capacity constraints are
  exercised. Invalid operations leave stored bytes and current state unchanged.
- Subprocess tests run the actual table CLI and protect managed files from raw
  mutations. Strict replay rejects semantically invalid history with valid CRC.
- The catalog/event AddressSanitizer smoke run completed 4,053,165 executions in
  16 seconds without a crash (configured budget: 15 seconds).
- Source size is 2992 physical Rust lines; 2743 excluding blank/comment-only lines.
  The two new code increments add 1003 and 990 physical lines respectively.

These are normal-reopen, validation and bounded fuzz checks. They do not prove
durability under process termination, power loss or torn page writes. Stage 2
remains open; no WAL, commit/rollback, checkpoint or transactional recovery exists.
