# Roadmap and acceptance gates

The minimal stage-1 core is implemented and tested. Later acceptance gates remain
open; a table engine is not a completed transaction engine or backend platform.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | implemented; normal reopen and validation tests pass |
| 2 | WAL, commit/rollback, checkpoint, locks | acknowledged commits survive kill; uncommitted writes absent; corruption matrix | in progress; process-kill, byte-cut, checkpoint and competing-writer checks pass; wider fault matrix open |
| 3 | original SQL lexer/parser/planner/executor, indexes | documented SQL subset and semantic tests | bounded SQL subset/CLI and standalone B+ tree tested; durable index integration and wider query checks pending |
| 4 | isolated projects, Axum REST, keys, limits | cross-project denial tests and graceful shutdown | registry/key rotation, scoped Axum routes, bounds and graceful shutdown tested; wider isolation/crash/load gates open |
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | backup foundation and project TypeScript SDK implemented; other platform features pending |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | experimental Docker/Compose tested; format upgrade/load/security gates open |

## Not supported

Durable table indexes, background journal rotation and history vacuuming,
Extended SQL, user/session authentication, row policies,
file uploads, realtime, migrations, incremental/encrypted backups, Kotlin SDK, web dashboard and production deployment.
No PostgreSQL compatibility guarantee. No production release. No real-data import.

## Next increments

1. Extend random crash/fault campaigns and backup publication I/O failures.
2. Design history vacuuming/retirement and extend format upgrade compatibility.
3. Integrate the bounded B+ tree with table/WAL allocation and atomic transaction replay.
4. Add Kotlin client SDK, then extend
   random network/media/publication campaigns and complete registry backup.

## Isolated registry and API keys

The synchronous server library publishes independent managed project directories,
persists checked version-1 metadata and hashes for 256-bit random keys, and rotates
credentials atomically. Single-use request capabilities preserve root ownership
and serialize same-project operations. Scope, traversal, symlink, permissions,
real capacity, concurrent request and metadata bounds execute. Passwords/sessions,
granular row authorization and complete registry backups remain pending.
See [project registry](projects.md) and [ADR 0012](adr/0012-isolated-projects.md).

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

An explicit stable arena now retains survivor IDs, reuses holes and exports
canonical root/revision/count snapshots. Bound atomic in-memory deltas validate
full resulting topology. Both modes pass exact compatibility, capacity and model
checks. Standalone private snapshot publication/CLI now passes sync, process-kill,
ownership and competing-writer checks. Table/WAL participation remains pending.

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
- Atomic checkpoint cache; damaged caches are ignored and explicit checkpoint regenerates them from WAL.
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

## HTTP transport increment

Scoped Axum REST, strict bounded JSON bodies, four blocking-work permits, async
registry waiting, peer attempt limits, redacted structured logs and graceful
shutdown are implemented. Actual socket/binary/restart and worker/body/rate tests
execute. OpenAPI describes only existing routes. Wider connection/load/security,
concurrent commit-disconnect and registry crash/backup gates remain open. See
[HTTP contract](server.md) and [ADR 0013](adr/0013-bounded-http-transport.md).

Registry publication kills/injected sync errors and actual HTTP writer kills,
concurrency, cancellation/drain and journal-loss isolation now execute. These
checks preserve ACKs but do not close physical-power-loss, full platform backup,
load or security acceptance gates.

A real TCP regression first reproduced private extension-method text entering
logs. Static standard/OTHER labels now pass accepted/denied token-shaped method
checks in both the actual server binary and rebuilt release container.

## TypeScript SDK increment

A dependency-free project client performs SQL/explain/status with strict runtime
validation, byte/shape bounds, exact safe integers and unknown-outcome errors.
Eleven unit and seven live server/restart cases run. It neither persists credentials
nor retries writes. npm publication, browser/CORS and Kotlin remain pending.

## Container deployment increment

The original compiled Rust server/CLI runs as a non-root process with a private
volume, read-only image root, loopback host port and applied cgroup-v2 limits.
Actual SQL/key isolation, offline verified backup/restore, WAL-2 compaction,
same-volume recreation, SIGKILL ACK preservation and journal-damage isolation
execute through the container. Full platform backups, arbitrary cross-version
upgrade, remote TLS, load/security and physical-power-loss gates stay open.
See [deployment](deployment.md) and [ADR 0015](adr/0015-experimental-containers.md).

## Offline registry-backup increment

Version-1 EMILYREG images include canonical project metadata and independently
verified database backups, preserving identities, rotated digests/epochs and both
WAL versions. Capture holds every database owner and refuses active capabilities.
Private no-clobber archive publication and whole-registry verified restore are
implemented through the CLI. Basic corruption, identity, format/bounds, ownership,
rejected-output preservation, properties and binary checks execute. This covers
the current registry, not unimplemented object/session services or unrelated
standalone indexes. Dedicated publication kills/injected sync failures, synchronized
restorers, generated cross-project histories, real capacity and restored binary/
container HTTP checks now execute. Wider failing-media/backup, power-loss,
security/load and stable-release acceptance remain open.
See [format](registry-backup-format.md) and [ADR 0018](adr/0018-offline-project-registry-backups.md).
