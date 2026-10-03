# Executed recovery checks and open gates

Linux, local temporary files, synthetic data, Rust stable 1.99.0, 2026-10-03.

| Boundary or failure | Observed result | Check |
| --- | --- | --- |
| Staged table changes, no WAL write, process killed | previous state only | integrated crash tests |
| Synced page frames without commit, process killed | previous state only; next commit removes tail | WAL and integrated crash tests |
| Synced table commit acknowledged, process killed | complete batch restored | integrated crash tests |
| Kill during continuous commits, four thresholds | every observed acknowledgment restored; history contiguous | streaming writer test |
| Every byte cut inside two-page batch | previous state until the complete commit frame exists | WAL and integrated recovery tests |
| Complete frame/page CRC damage | error; input preserved; no stale-checkpoint fallback | WAL and integrated corruption tests |
| Valid CRC with invalid IDs, targets or events | error; earlier committed events cannot be rewritten | codec/history/event tests |
| Checkpoint synced before rename, process killed | all committed rows recovered from WAL | checkpoint subprocess test |
| Checkpoint renamed before directory sync, process killed | all committed rows recovered from WAL | checkpoint subprocess test |
| Damaged/partial checkpoint | state restored from WAL, cache regenerated | transaction file tests |
| Four competing owners, 20 updates each | all 80 increments restored | process concurrency test |
| OS rejects page/commit write | writer poisoned; ambiguous commit reports transaction ID | read-only-handle failure tests |
| Short writes or interrupted syscall | complete acknowledged batch restored | deterministic WAL I/O tests |
| Disk full in page/commit frame, zero write | prior ACKs survive; partial tail ignored; owner poisoned | deterministic WAL I/O tests |
| Sync failure before/after underlying sync | no ACK; complete commit may exist; inspect after reopen | deterministic WAL I/O tests |
| Uncommitted sync, rollback truncate or tail sync fails | no later commit accepted by poisoned owner | deterministic WAL I/O tests |
| Source export read fails | source bytes preserved; owner requires reopen | deterministic WAL I/O tests |
| Every archive byte mutation/truncation | verification fails before restore output exists | backup archive tests |
| Rehashed archive with valid CRCs but invalid table history | verification/restore fail closed | backup history tests |
| Backup/restore staged and synced, process killed | final path absent | publication subprocess matrix |
| Backup/restore published before parent sync, process killed | complete output verified/reopened | publication subprocess matrix |
| Two competing backup/restore publishers | one complete winner; existing output preserved | publication race tests |
| Parent open fails after restore rename | complete output retained; durability reported unknown | publication regression test |
| Generated CRUD, failed writes and rollback before backup | restored rows match independent map; new commits work | 32-case backup model property |

Helper tests are ignored in the parent run and invoked explicitly in children.
They are not counted as separate successful coverage cases. Checkpoint barriers
and backup publication barriers are private callbacks; production has no environment
variables that pause writes. Injected I/O errors wrap real locked files; they do
not emulate a hardware power failure.

A commit can become durable before its response is observed. The streaming test
allows additional complete committed transactions after the last observed ACK.
It forbids lost ACKs, gaps and replay of uncommitted page frames. Exactly-once
retries need a future request/idempotency protocol.

## Still open

- Actual power interruption and hardware/filesystem behavior beyond sync calls.
- Wider backup-publication I/O fault injection and failing-media behavior.
- Journal rotation, checkpoint reuse and retirement ordering.
- Long random crash campaigns and failing-media recovery.
- Backup upgrade compatibility and stable-format migration.
- Security audit, server-level project isolation and authorization.

Stage 2 remains **in progress**. These checks do not make the platform production-ready.
