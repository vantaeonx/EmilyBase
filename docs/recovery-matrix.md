# Executed recovery checks and open gates

Linux, local temporary files, synthetic data, Rust stable 1.99.0, updated 2026-10-04.

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
| Compaction staged, renamed, directory-synced or returned, process killed | exact acknowledged state restored for both source versions | 8-boundary subprocess matrix |
| Compaction parent sync fails before/after underlying sync | complete selected baseline; owner poisoned; reopen required | 4-case fault matrix |
| WAL reaches actual 64 MiB cap, versions 1/2 | failed write changes no bytes; compaction and subsequent commits work | capacity integration test |
| Compacted commit/tail or streaming writer killed | all observed ACKs retained; uncommitted frames absent | version-1/2 table crash matrix |
| Four competing owners with 16 compactions | all 80 increments and transaction IDs preserved | process concurrency test |
| Missing/damaged baseline with valid staging/cache | error; no fallback or input mutation | baseline selection tests |
| Valid CRC/digest with invalid baseline/append history | error; constraints and old records protected | baseline relational tests |
| Existing live snapshot differs from valid persisted bytes | export/compaction refuses; owner poisoned | live consistency regression |
| Frozen version-1 root/table bytes | identical normalized SHA-256 after new code and reopen | compatibility fixture test |
| Every baseline byte cut or mutation | error; no incomplete baseline recovery | version-2 WAL tests |
| Compacted histories with later writes | rows/history/IDs preserved; next transaction continues | compaction file/model tests |
| More than 256 baseline pages | all 300 rows restored; normal transaction limit unchanged | baseline/integrated tests |
| WAL inode replaced while owner exists | another database owner remains excluded | directory ownership regression |
| Version-1 and version-2 backup payloads | independent verified restore and new writes | backup compatibility tests |
| Backup/restore parent or staged entry substituted before rename | refusal; foreign entries untouched; source bytes unchanged | reproduced regressions and eight external subprocess changes |
| Backup/restore file/WAL/staging sync fails before/after real fsync | no selected target; owned staging cleaned | twenty-case combined publication sync matrix |
| Backup/restore parent sync or identity fails after rename | complete selection preserved; unknown publication outcome | sync and post-publication identity cases |
| Final archive symlink, FIFO, device or directory input | refusal without blocking/read | no-follow/nonblocking regular-input tests |
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
- Wider compaction filesystem faults, history vacuuming and retirement behavior.
- Long random crash campaigns and failing-media recovery.
- Backup upgrade compatibility and stable-format migration.
- Security audit, user/row authorization and broader server/load isolation campaigns.

Stage 2 remains **in progress**. These checks do not make the platform production-ready.


## Server and project publication matrix

| Fault/operation | Required observed result | Executed check |
| --- | --- | --- |
| Registry backup/restore parent or staging replaced before rename | refusal; foreign entries and exact source preserved | two reproduced regressions, identity/alias and native cases |
| Ancestor replaced between restore WAL sync and later project writes | original private staging used; replacement subtree untouched | pinned-directory restore regression |
| Registry parent/selection replaced after rename | complete original retained; unknown publication outcome | native and unit pre/post-rename cases |
| Repeated registry refusal/uncertainty | no leaked owned descriptors after each cycle | 64 isolated native cycles |
| Project staging/data/metadata sync, killed | incomplete project never listed; old data unchanged | six creation kill boundaries |
| Project rename/root sync/returned key, killed | complete project opens; acknowledged key works | creation publication/ACK kills |
| Rotation file sync/rename/dir sync/ACK, killed | one complete key epoch; old WAL unchanged | four boundaries on both WAL versions |
| Creation/rotation directory sync failure | pre-publication refusal or poisoned uncertain owner; reopen required | five injected sync boundaries |
| HTTP writer killed on response counts 5/20/60 | every complete response retained; atomic gapless prefix | six binary/TCP kills, WAL 1/2 |
| Rolled-back/rejected SQL followed by kill | exact prior WAL, no staged rows | actual HTTP rollback kill |
| Four TCP clients, two projects | 32 scoped rows and per-project IDs without lost writes | concurrent network test |
| Client cancellation after blocking work starts | permit/root owner retained; commit finishes | controlled blocking-boundary test |
| SIGTERM while accepted body incomplete | request drains, commit recovered, owner then released | actual binary/TCP drain |
| Missing/corrupt project WAL with old cache | generic 503; no fallback/mutation; sibling works | actual HTTP corruption tests |
| Damaged optional checkpoint | WAL-backed reads work; explicit checkpoint alone repairs cache | HTTP/cache test |

Registry kill/fault callbacks compile only in unit-test builds. Private synthetic
fixture keys stay in disposable 0600 files and are never printed or committed.

## Executed container checks

| Operation | Observed result | Check |
| --- | --- | --- |
| Same-volume container recreation after WAL-2 compaction | keys, rows and last transaction preserved | real release image/Compose probe |
| Offline backup, verify, restore and new restored write | exact synthetic rows/schema/ID; original preserved | image's compiled CLI |
| SIGKILL during response-counted writes | every received ACK survives; complete gapless prefix; later writes work | real container writer |
| Corrupt one project journal | that project fails closed; healthy sibling remains available | disposable container fault |
| Rootless cgroup-v2 runtime limits | actual memory/CPU/process controller files match configuration | live container inspection |

Same-revision recreation is not a future-format upgrade acceptance test. The probe
deletes only its randomly named synthetic resources. Future platform-object backups,
physical power loss, security and wider load campaigns remain open.

## Standalone index snapshot publication

| Boundary | Observed result | Check |
| --- | --- | --- |
| Creation file/stage synced, killed | final directory absent | two process kills |
| Creation renamed/parent synced/ACK, killed | complete revision one, new writes work | three process kills |
| Replacement file synced, killed | previous complete revision | process kill |
| Replacement renamed/directory synced/ACK, killed | complete selected next revision | three process kills |
| Ten before/after sync failures | old selection or reported unknown complete publication | injected sync matrix |
| Two competing process owners | 20 increments, revision 21 and exact final pointer | real subprocess writers |
| Owned root moved/replaced or permissions widened | refuse and poison; neither directory modified | reproduced path regression |
| Generated operations with reopen after each | exact model rows, bytes and revisions | 32-case persistent property |

The publisher is independent of table/WAL transactions. No integrated durable
index, hardware power-loss or concurrent-reader acceptance gate closes.

## Whole-registry archives and independent restored service

| Boundary | Observed result | Check |
| --- | --- | --- |
| All source owners acquired, killed | no archive; source exact; every owner released | two controlled kills including ownership probe |
| Archive file synced, killed | final archive absent; staging not adopted | process kill |
| Archive renamed/parent synced/returned ACK, killed | exact complete archive and usable independent restore | three process kills |
| Restore WAL/project/staging synced, killed | final registry absent; fresh restore still works | three process kills |
| Restore renamed/parent synced/returned ACK, killed | complete scopes/epochs/IDs and later writes | three process kills |
| Fourteen before/after sync failures | unchanged output or explicitly unknown complete publication | injected real-sync matrix |
| Two synchronized process restorers | exactly one complete winner | subprocess race |
| Generated cross-project changes/rotation/compaction/rollback/cache damage | repeated restored prefixes equal independent model | 24-case property |
| Actual 128-project source | all keys restored; project 129 refuses | capacity integration |
| Oversized aggregate sparse WALs | refusal before engine opens; no archive | aggregate-boundary case |
| Real restored HTTP with a separate external master | scoped rows/rotation/writes; original exact; restored ACK survives kill | binary/TCP integration |
| Actual release container serving the registry copy | retained credentials, isolated rows and independent writes | real Compose probe |

These checks cover the current registry, including plaintext metadata/digests and
mandatory histories. Unimplemented object/session services and unrelated standalone
indexes are outside the archive; broad failing-media and production gates remain open.

## Directory identity regression

Two failing cases first reproduced project creation following a replaced root and
accepted capabilities following replacement directories. Pinned no-follow handles
now reject root/project/data device/inode changes. Nine accepted-capability cases
cover status, explain and execution at all three levels. A real TCP/binary matrix
checks generic 503s for SQL/explain/status, listing and rotation, rejected creation
on a replaced root, preserved old/replacement bytes and healthy sibling access.
Stopping/reopening intentionally moved directories works. Digest authorization
remains filesystem-free; synchronous checks run on blocking workers. This does
not sandbox a malicious privileged administrator or close the wider audit gate.
