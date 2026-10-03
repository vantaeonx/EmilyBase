# Roadmap and acceptance gates

The minimal stage-1 core is implemented and tested. Later acceptance gates remain
open; a table engine is not a completed transaction engine or backend platform.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | implemented; normal reopen and validation tests pass |
| 2 | WAL, commit/rollback, checkpoint, locks | acknowledged commits survive kill; uncommitted writes absent; corruption matrix | WAL and managed transactions implemented; table crash matrix pending |
| 3 | original SQL lexer/parser/planner/executor, indexes | documented SQL subset and semantic tests | not started |
| 4 | isolated projects, Axum REST, keys, limits | cross-project denial tests and graceful shutdown | not started |
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | not started |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | not started |

## Not supported

B+ tree, journal rotation,
SQL, joins, indexes, project isolation, server, authentication, row policies,
file uploads, realtime, migrations, backups, SDKs, web dashboard and deployment.
No PostgreSQL compatibility guarantee. No production release. No real-data import.

## Next increments

1. Expose managed-directory transactions through CLI; preserve explicit legacy mode.
2. Crash harness: termination before/after WAL sync, page sync and checkpoint.
3. Corruption, concurrent-process and backup/restore acceptance matrices.

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

Lock and creation-race tests cover initial file ownership. They do not satisfy
the concurrent-transaction or forced-termination recovery gates in stage 2.

Only update a gate to complete when its full criteria have executed successfully.
