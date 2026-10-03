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
cargo +nightly fuzz run wal_records -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
cargo +nightly fuzz run managed_recovery -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
```

The target checks raw headers, pages and pages with a repaired checksum to reach
structural validation. Add valid synthetic header/page seeds to
`fuzz/corpus/file_format` for better coverage. Corpus and crash artifacts are
ignored. A bounded smoke run is not a complete fuzz campaign or a security audit.
The catalog target checks schema, row and relational-event codecs and round trips
accepted records. Synthetic `ESCH`, `EROW` and `ETBL` seeds improve its coverage.

## Pending acceptance tests

Core process-kill, byte-cut, checkpoint and competing-writer checks now execute.
Broader I/O fault injection, power-loss, backup/restore and cross-project
authorization remain open. See the [current matrix](recovery-matrix.md).

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
remains open; this table increment has no integrated transactional recovery.

## Standalone WAL increment: executed checks

On 2026-10-03, workspace formatting, Clippy with warnings denied, build and all
76 tests passed. The new block contains 998 physical Rust lines. It exercises
every byte cut within a two-page batch, valid checksums with invalid semantics,
64-case properties, bounded allocations, owner-only permissions, lock ownership,
explicit rollback and dropped pending batches. A subprocess is forcibly killed
after a synced commit and after synced uncommitted page frames; reopen preserves
the former and excludes the latter. The process tests use a separate executable
to avoid transient file-lock inheritance by another test's fork/exec.

The WAL AddressSanitizer smoke run completed 1,554,154 executions in 16 seconds
without a crash (configured budget: 15 seconds). Total Rust source size is now
3990 physical lines. Table integration, checkpoint crashes, concurrent transaction
sequencing, backup/restore and real power-loss testing remain open.

## Managed transaction increment: executed checks

On 2026-10-03, formatting, Clippy with warnings denied, build and all 93 tests
passed. This block adds 996 physical Rust lines; the source total is 4986.
Multi-table batches, strict abort-on-write-error, rollback/drop, no-op commits,
page spill, checkpoint replacement, owner-only permissions and identity checks
are exercised. Recovery rejects rewritten history and invalid relational events
even when journal and page CRCs are valid. Read-only OS handles reproduce real
write failures and verify poisoned writers and unknown commit outcomes.

A 32-case persistent transaction property compares generated committed/rolled-back
batches against an independent map and reopens after each batch. Checkpoint damage
and leftover temporary files cannot alter committed rows. These tests do not yet
cover killing the integrated table engine during commit/checkpoint or a full
concurrency, backup/restore and power-loss matrix.

## Integrated recovery and CLI increment: executed checks

On 2026-10-03, workspace/fuzz formatting, Clippy with warnings denied, build and
all 105 main tests passed. Four subprocess helpers are ignored in the parent
runner and invoked by real kill/concurrency tests. This block adds 976 physical
Rust lines; the total is 5962. CLI tests execute managed CRUD, every batch operation,
rollback, abort, limits, value-redacted errors and explicit legacy compatibility.

Integrated recovery checks all 12480 cut positions inside a two-page transaction.
The process matrix kills staged writes, synced uncommitted frames, acknowledged
commits, continuous writers at four thresholds, and checkpointing before/after
rename. Four competing processes restore all 80 increments without lost updates.
Missing/corrupt WAL fails closed even when a stale checkpoint exists.

The managed-recovery AddressSanitizer smoke run completed 587,298 executions in
16 seconds without a crash (configured budget: 15 seconds). The target also repairs
nested checksums and commit digests to reach relational validation. The full
reliability and security gates remain open; see the recovery matrix.
