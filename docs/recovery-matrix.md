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

Helper tests are ignored in the parent run and invoked explicitly in children.
They are not counted as separate successful coverage cases. Checkpoint barriers
are private callbacks; production has no environment variables that pause writes.

A commit can become durable before its response is observed. The streaming test
allows additional complete committed transactions after the last observed ACK.
It forbids lost ACKs, gaps and replay of uncommitted page frames. Exactly-once
retries need a future request/idempotency protocol.

## Still open

- Actual power interruption and hardware/filesystem behavior beyond sync calls.
- Injected short writes, disk exhaustion and synchronization errors.
- Journal rotation, checkpoint reuse and retirement ordering.
- Long random crash campaigns and failing-media recovery.
- Verified backup/restore, upgrades and stable-format migration.
- Security audit, server-level project isolation and authorization.

Stage 2 remains **in progress**. These checks do not make the platform production-ready.
