# Roadmap and acceptance gates

The minimal stage-1 core is implemented and tested. Later acceptance gates remain
open; a table engine is not a completed transaction engine or backend platform.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | implemented; normal reopen and validation tests pass |
| 2 | WAL, commit/rollback, checkpoint, locks | acknowledged commits survive kill; uncommitted writes absent; corruption matrix | in progress; process-kill, byte-cut, checkpoint and competing-writer checks pass; wider fault matrix open |
| 3 | original SQL lexer/parser/planner/executor, indexes | documented SQL subset and semantic tests | bounded SQL subset/CLI and standalone B+ tree tested; durable index integration and wider query checks pending |
| 4 | isolated projects, Axum REST, keys, limits | cross-project denial tests and graceful shutdown | not started |
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | backup foundation implemented; other platform features not started |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | not started |

## Not supported

Durable table indexes, background journal rotation and history vacuuming,
Extended SQL, project isolation, server, authentication, row policies,
file uploads, realtime, migrations, incremental/encrypted backups, SDKs, web dashboard and deployment.
No PostgreSQL compatibility guarantee. No production release. No real-data import.

## Next increments

1. Extend random crash/fault campaigns and backup publication I/O failures.
2. Design history vacuuming/retirement and extend format upgrade compatibility.
3. Integrate the bounded B+ tree with table/WAL allocation and atomic transaction replay.
4. Add the isolated-project server with strict authorization, bounded worker calls
   and synthetic-data-only deployment; continue wider recovery/query campaigns.

## SQL parser increment

The original bounded lexer/parser/AST accepts DDL, CRUD, predicates, ordering,
limits, one inner join and transaction control. It enforces input/token/statement,
literal/identifier/parameter and expression-depth bounds, without echoing input.
Two 64-case properties and bounded ASan fuzzing execute. Schema-resolved plans,
bounded scans/joins, null logic, atomic script execution and CLI now follow the
parser. A separate 32-case CRUD/reopen model exercises committed SQL batches.
Dedicated SQL writer kills, backup/restore for both WAL versions, actual row/memory
bounds and execution ASan checks now run. Two 48-case read/join models verify
projection, null ordering, filtering and limits against independent rows.
Durable index integration and wider query gates remain open. See [SQL grammar](sql.md) and
[ADR 0011](adr/0011-bounded-sql.md).

## Index foundation

The original `index` crate has versioned 4096-byte leaf/internal images, checksums,
numeric/UTF-8 keys, unique insertion, recursive splits, pointer replacement,
deletion with rotations/merges/root collapse, sorted bulk loading, point lookup
and linked-leaf range scans. Imports validate complete topology, exact separators, occupancy,
balanced leaf depth, reachability and successor links. Errors preserve staged tree
state. Boundary, capacity, independent-model property and parser fuzz checks execute.
This library does not yet persist through the managed transaction engine. Existing
table files and their primary-key maps are unchanged. Successful deletion can
renumber index arena IDs; external row pointers remain unchanged. See
[ADR 0009](adr/0009-bounded-index-foundation.md) and
[ADR 0010](adr/0010-index-maintenance.md).

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
- Mandatory self-contained WAL with strict append-only relational recovery.
- Explicit version-2 baseline compaction, stable directory ownership and transaction ID preservation.
- Complete baseline cuts/corruption fail closed; version-1 files/backups remain readable.
- Compaction kill boundaries, unknown directory-sync outcomes and capacity recovery.
- Four competing owners perform 80 updates with 16 compactions; all updates survive.
- Frozen synthetic version-1 bytes and canonical/all-type compaction properties.
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
