# Roadmap and acceptance gates

All implementation stages are initially incomplete.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | workspace only |
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

1. Fixed-size pages, bounded decoder, pager and useful CLI commands.
2. Catalog, basic values, schemas and primary-key CRUD with integration tests.
3. WAL before making any transactional durability claim.
4. Crash harness: termination before/after WAL sync, page sync and checkpoint.
5. Corruption, concurrent-process and backup/restore acceptance matrices.

Only update a gate to complete when its full criteria have executed successfully.
