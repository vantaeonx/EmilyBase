# Roadmap and acceptance gates

The minimal stage-1 core is implemented and tested. Later acceptance gates remain
open; a table engine is not a completed transaction engine or backend platform.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | implemented; normal reopen and validation tests pass |
| 2 | WAL, commit/rollback, checkpoint, locks | acknowledged commits survive kill; uncommitted writes absent; corruption matrix | in progress; process-kill, byte-cut, checkpoint and competing-writer checks pass; wider fault matrix open |
| 3 | original SQL lexer/parser/planner/executor, indexes | documented SQL subset and semantic tests | not started |
| 4 | isolated projects, Axum REST, keys, limits | cross-project denial tests and graceful shutdown | not started |
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | backup foundation implemented; other platform features not started |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | not started |

## Not supported

B+ tree, journal rotation,
SQL, joins, indexes, project isolation, server, authentication, row policies,
file uploads, realtime, migrations, incremental/encrypted backups, SDKs, web dashboard and deployment.
No PostgreSQL compatibility guarantee. No production release. No real-data import.

## Next increments

1. Design checkpoint reuse / journal rotation with durable metadata and crash tests.
2. Extend backup publication I/O failures, long random crash campaigns and upgrade tests.
3. Build the original B+ tree and query layer after documenting their bounded behavior.

## Implemented first increment

- Original versioned header and 4096-byte slotted pages with CRC32.
- Bounded decoding, compaction and physical record mutation.
- No-clobber file creation, sync points and exclusive advisory ownership.
- CLI and unit, integration, subprocess and property tests.
- A format fuzz target and GitHub Actions for Rust checks.
- Typed catalog, schema/key validation and bounded binary row/event codecs.
- Atomic publication with initialized pages and a table root marker.
- Table create/drop, typed row CRUD, primary-key uniqueness and bounded scans.
- Strict table-history replay and rejection of invalid event sequences.
- Reopen-after-each-operation property tests against an independent row model.
- CLI protection against raw mutations of managed table databases.

## Implemented reliability increments

- Bounded full-page WAL with transaction/sequence IDs, commit digest and sync ordering.
- Staged table transactions, commit/rollback and abort-on-write-error.
- Mandatory retained WAL with strict append-only relational recovery.
- Atomic checkpoint cache; damaged caches are regenerated from WAL.
- Managed CLI mode, transaction batches and explicit legacy compatibility.
- Process kills before/after commit, streaming writes and checkpoint rename.
- Four competing processes, byte-cut matrix, OS write failures and recovery fuzzing.
- Deterministic short-write, interrupted-write, disk-full, truncate/read and sync failures.

Remaining gates are listed in the [recovery matrix](recovery-matrix.md).

## Verified backup increments

Committed-WAL archives, bounded format validation, SHA-256 and strict relational
replay are implemented. Restore stages and verifies a working database before
atomic no-replace directory publication. Existing outputs are preserved. CLI,
backup/restore process-kill boundaries, competing destinations, generated CRUD
restore models and archive fuzzing execute. Upgrades, physical power loss and
broader publication I/O injection remain open.

Only update a gate to complete when its full criteria have executed successfully.
