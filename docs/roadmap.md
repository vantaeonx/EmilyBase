# Roadmap and acceptance gates

All implementation stages remain incomplete. The first stage has a working
page-storage increment; this is not a completed table or transaction engine.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | pages, typed schemas and codecs implemented; table persistence pending |
| 2 | WAL, commit/rollback, checkpoint, locks | acknowledged commits survive kill; uncommitted writes absent; corruption matrix | not started |
| 3 | original SQL lexer/parser/planner/executor, indexes | documented SQL subset and semantic tests | not started |
| 4 | isolated projects, Axum REST, keys, limits | cross-project denial tests and graceful shutdown | not started |
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | not started |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | not started |

## Not supported

Tables, typed rows, primary keys, transactions, WAL, checkpointing, B+ tree,
SQL, joins, indexes, project isolation, server, authentication, row policies,
file uploads, realtime, migrations, backups, SDKs, web dashboard and deployment.
No PostgreSQL compatibility guarantee. No production release. No real-data import.

## Next increments

1. Catalog, basic values, schemas and primary-key CRUD with integration tests.
2. WAL before making any transactional durability claim.
3. Crash harness: termination before/after WAL sync, page sync and checkpoint.
4. Corruption, concurrent-process and backup/restore acceptance matrices.

## Implemented first increment

- Original versioned header and 4096-byte slotted pages with CRC32.
- Bounded decoding, compaction and physical record mutation.
- No-clobber file creation, sync points and exclusive advisory ownership.
- CLI and unit, integration, subprocess and property tests.
- A format fuzz target and GitHub Actions for Rust checks.
- Typed catalog, schema/key validation and bounded binary row/event codecs.
- Atomic publication with initialized pages for a future table root marker.

Lock and creation-race tests cover initial file ownership. They do not satisfy
the concurrent-transaction or forced-termination recovery gates in stage 2.

Only update a gate to complete when its full criteria have executed successfully.
