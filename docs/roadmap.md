# Roadmap and acceptance gates

2026-10-06: opt-in physical plan/replay allocation samples are preserved with
bounded version-2 report admission. They expose full-history reconstruction costs
and release behavior; numeric transient admission and durable writer gates remain
open. See [observations](image-replay-profiles.md) and
[ADR 0045](adr/0045-image-plan-and-replay-allocation-diagnostics.md).

The subsequent [shared history append](shared-history-replay.md) reduces replay
copying while preserving full plan/root/state validation. It passes independent
append/full-recovery/row-model checks and retains exact refusal/release gates.
Byte/transient reservations and the combined durable writer remain pending under
[ADR 0046](adr/0046-shared-tail-history-replay.md).

Optional [parallel observations](parallel-replay-profiles.md) now launch four
real scoped replay workers and check cancellation, ordering and group refusal.
This is diagnostic evidence, not server integration or worst-case byte admission;
those gates remain open under [ADR 0047](adr/0047-scoped-parallel-replay-observations.md).

The minimal stage-1 core is implemented and tested. Later acceptance gates remain
open; a table engine is not a completed transaction engine or backend platform.

| Stage | Scope | Acceptance gate | Status |
| --- | --- | --- | --- |
| 0 | design, threats, format, ADRs | documents reviewed against implementation | initial documents |
| 1 | pages, tables, types, primary keys, CRUD, CLI | unit/integration tests and reopen round trips | implemented; normal reopen and validation tests pass |
| 2 | WAL, commit/rollback, checkpoint, locks | acknowledged commits survive kill; uncommitted writes absent; corruption matrix | in progress; process-kill, byte-cut, checkpoint and competing-writer checks pass; wider fault matrix open |
| 3 | original SQL lexer/parser/planner/executor, indexes | documented SQL subset and semantic tests | SQL subset/CLI, derived B+ primary lookup and standalone publisher tested; durable index/secondary DDL and wider query gates pending |
| 4 | isolated projects, Axum REST, keys, limits | cross-project denial tests and graceful shutdown | registry/key rotation, scoped Axum routes, bounds and graceful shutdown tested; wider isolation/crash/load gates open |
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | backup foundation, TypeScript SDK, password helper and local private account store implemented; local durable sessions and coordinated offline root backup/restore implemented; explicit native account/session mode tested on real TCP with WAL1/2; private v4 policy persistence/current borrowed decisions and synchronous owned typed CRUD implemented; native filtered pages and trusted two-credential backend HTTP implemented; admitted user-only sessions/typed rows and explicit user SDK implemented; admitted own-password change verified with original epoch revocation; native object envelope/publication/inspection and retained project directories implemented; signup/roles/object HTTP/realtime/dashboard remain open |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | experimental Docker/Compose tested; format upgrade/load/security gates open |

## Not supported

Durable table indexes, background journal rotation and history vacuuming,
Extended SQL, public signup, roles and broader policy orchestration,
file uploads, realtime, online/schema-diff migrations, incremental/encrypted backups, Kotlin SDK, web dashboard and production deployment.
No PostgreSQL compatibility guarantee. No production release. No real-data import.

## Next increments

The [password verifier foundation](password-verifiers.md) supplies synchronous
fixed-policy Argon2id, strict records, shared bounded workspaces and synthetic
original-engine persistence/restore checks. It is a library helper, not user
registration or HTTP login. Private per-project accounts, bounded blocking-worker
integration, rate/enum controls, sessions, roles and row policies remain open under
[ADR0067](adr/0067-bounded-password-verifiers.md). No platform milestone is closed.

[ADR0068](adr/0068-private-project-account-store.md) adds local private scoped
account storage over the original engine, positive credential epochs, disable
state and verified local backup/restore. It remains separate from public SQL and
the live registry. Complete account/data capture, private authorized paths,
network admission and account policy are gates before HTTP
enablement. Current registry archives do not silently claim to contain it.

[ADR0069](adr/0069-purpose-bound-token-primitives.md) supplies purpose-bound
random tokens, strict fixed formats and independent verifier oracles.
[ADR0070](adr/0070-explicit-private-session-schema.md) adds explicit atomic
private v1-to-v2 migration and bounded family/reference validation.
[ADR0071](adr/0071-durable-session-time-watermark.md) adds private v3 time metadata,
forward observation and atomic incarnation/time reset. These intermediate
increments granted no session admission by themselves.

[ADR0072](adr/0072-durable-local-session-lifecycle.md) integrates these foundations
into real synchronous sign-in, current-state access verification, atomic refresh,
logout, trusted revocation and bounded cleanup. Independent sequence, concurrent
refresh and forced-kill/verified-restore tests execute. Current private restoration
still requires an explicit reset before traffic. The library is not attached to
server projects; HTTP workers/rate controls, account policy, complete account/data
backup/restore, roles and row policies remain gates. No milestone is closed.

[ADR0073](adr/0073-private-restore-reset-before-publication.md) adds owned private
restore preparation, semantic project/account validation and mandatory scope/time
reset before directory publication. Current private archives can be restored with
old-token denial from the first published state. Combined registry/account capture
and restoration remain the next integration gate; no HTTP route or milestone closes.

[ADR0074](adr/0074-owned-verified-private-archive-inventory.md) adds owned verified
backup snapshots, pure private inventory reports and common validation before
private file/image export. A future combined capture must retain every data/account
owner before its first prefix; independently appended images do not establish that
boundary. Combined capture/publication, server authentication and memory gates remain open.

[ADR0075](adr/0075-common-registry-private-capture.md) implements common offline
capture of registry data plus an explicit private roster while retaining every
source owner before the first prefix through final validation. The new EMILYBND-1
inspector checks complete nested schemas/scopes and database identity uniqueness.
Subset rosters are explicit. Combined root restoration,
authoritative roster, HTTP accounts and numeric memory gates remain open.


[ADR0076](adr/0076-owned-account-bundle-file-publication.md) adds private owned bundle
file publication and real count-only CLI inspection using the tested registry file
publisher. No-replace/readback/fsync, substitutions, native kills, competing file
publishers, independent row model and real CLI credentials/immutability checks
execute. Combined root restore/reset, automatic private roster and HTTP users
remain separate gates; no production milestone closes.

[ADR0077](adr/0077-restore-private-byte-images.md) adds direct original-engine and
private byte-image restoration using shared owned preparation/publication. The
private wrapper resets scope before selection; no intermediate sensitive input
archive file is required. File/byte version matrices, independent account-state
model and additional native kill modes execute. Combined root coordination remains
open; no platform or production milestone closes.

[ADR0078](adr/0078-atomic-account-bundle-root-restore.md) coordinates offline registry
and explicit private roster restoration under one root. Every private scope resets
before selection; exact prepared histories and descriptor-owned inventories are
verified. The experimental root manifest records selected scope/time without granting
authority. Automatic HTTP account attachment/roster and production gates remain open.

[ADR0079](adr/0079-offline-root-operator-cycle.md) closes the explicit offline
CLI restore/verify/re-backup cycle. Root capture retains every data/private owner
through full inventory validation and preserves source credentials. Required trusted
reset time is bounded and invalid values are never echoed. No HTTP account worker,
automatic private discovery, whole-process memory or production gate closes.

[ADR0080](adr/0080-retained-private-root-service.md) retains the exact inspected
registry/private owners for synchronous project-key-gated lifecycle operations.
Normal restart preserves scope/time/families; four active private stores are
admitted before database opening. Network workers/rate/body/DTO admission, public
account policy, dynamic roster and resource/production gates remain open.

[ADR0081](adr/0081-private-root-http-mode.md) supplies an embedded fixed-root HTTP
transport with current service-key admission, bounded bodies/workers/rates,
trusted system time and explicit no-cache secret responses. In-process actual
WAL/middleware checks execute; ADR0082 below adds the subsequent native executable
mode and process-level HTTP restart/kill evidence. Public user data access stays closed.

The declared Linux Rust floor is verified on 1.89.0 with the current frozen
[workspace checks](testing.md);
CI adds a dedicated minimum-toolchain job alongside stable/SDK and real Docker
checks. This is build/test compatibility, not a stable file-format upgrade gate.

1. Extend random crash/fault campaigns and backup publication I/O failures.
   Single-database publication now pins destination/staging identities; reproduced
   substitutions, native process changes, before/after fsync failures and generated
   verified restore execute. Wider failing-media campaigns remain open. See
   [ADR 0032](adr/0032-owned-backup-publication.md).
   The independent registry publisher is also bound to original handles, with
   reproduced substitutions, native pre/post-rename checks, descriptor-release
   cycles and an independent scope/epoch model. See
   [ADR 0033](adr/0033-owned-registry-backup-publication.md).
   Raw page creation now pins and verifies exact file/directory ownership; managed
   checkpoints use their existing directory handle. Readback, three creation kills,
   sync faults, a generated page model and both-WAL namespace regressions execute.
   See [ADR 0034](adr/0034-owned-page-file-publication.md).
   Explicit WAL replacement also uses the held directory and checks authoritative
   selection, staging and post-rename exact bytes; uncertain selection poisons the
   owner. Native substitutions and an independent relocation model exercise it.
   See [ADR 0035](adr/0035-owned-journal-replacement.md).
   Managed creation/opening now bind the directory and mandatory WAL together,
   with named identity checks, private creation and no-follow leaf admission.
   Detectable partial initialization, native kills/substitutions, sync failures
   and independent row models remain separate from hardware power-loss acceptance.
   See [ADR 0036](adr/0036-owned-database-initialization.md).
2. Design history vacuuming/retirement and extend format upgrade compatibility.
3. Integrate the bounded B+ tree with table/WAL allocation and atomic transaction replay.
   [ADR 0031](adr/0031-atomic-index-wal-proposal.md) proposes the commit/domain
   boundary and required evidence; no new format or migration is enabled.
   The standalone namespace/root codecs are implemented separately in
   `commit-format`; runtime WAL selection remains unchanged. The
   staged table/index state model is now implemented in a separate `commit-model`
   prototype, with an independent generated reference row map. Full 10000-row
   integer/short-text/long-text capacity cases execute; combined encoded/heap
   budgets, typed durable records and recovery/migration acceptance remain
   before writer enablement. See [ADR 0037](adr/0037-experimental-commit-namespaces.md)
   and [ADR 0038](adr/0038-staged-table-index-model.md), plus
   [capacity arithmetic](durable-index-capacity.md).
   The model's 2048-page aggregate admission, component reports, canonical hash
   streaming/reuse and full 128-table fragmented-capacity checks now execute.
   These cover image counts, not heap/replay/worker/WAL reservation. See
   [ADR 0039](adr/0039-combined-model-index-images.md).
   Opt-in release allocation diagnostics now measure real synthetic stages,
   retained views and full-versus-streamed fingerprints. Four held long-key
   models peak near 940 MiB in one local shape; serial construction does not
   measure concurrent worker transients. Heap/lifetime admission remains open.
   See [ADR 0040](adr/0040-opt-in-model-allocation-diagnostics.md) and
   [profiles](model-allocation-profiles.md).
   Relational snapshot clones now share tables/location maps and detach the
   affected table on first write. Read-only/index-only stages avoid eager row
   copies, while writes to a large table still copy that table. Lifetime/worker
   reservation remains open. See [ADR 0041](adr/0041-shared-relational-snapshot-tables.md).
   Detached maps now share unchanged keys/row bodies too; full-boundary identity,
   owned result isolation and release checks accompany the lower synthetic peak.
   Structural map copies, retained generations and numeric admission remain open.
   See [ADR 0042](adr/0042-shared-row-bodies-and-live-keys.md).
   Optional model-pool admission now reserves distinct retained/pending generations
   and limits reader objects, global writers and one writer per project. Refusal,
   release, descendant-held identity and actual thread contention are tested.
   Numeric heap/transient/replay/server admission is still open; no durable gate
   is completed. See [ADR 0043](adr/0043-bounded-model-lifetimes.md).
   Raw prepared states now materialize typed changed physical components and
   independently replay both projections against exact base/next fingerprints.
   Dense 10000-row/768-index-image and 256-appended-history-image cases keep
   domain bounds separate. No wire framing or durable writer is enabled. See
   [ADR 0044](adr/0044-validated-physical-image-plans.md).
4. Add Kotlin client SDK, then extend
   random network/media/publication campaigns and registry backup streaming/encryption.

Borrowed double-ended B+ intervals are now implemented, with bounded ancestor
paths, fused errors, unchanged EBIX-1 bytes and actual standalone range CLI.
Capacity, mixed keys, separator/leaf edges, alternating consumption, stable holes,
root collapse/reuse, generated mutation/import models and ASan checks execute.
Borrowed table-row integration and primary ordering are implemented below;
independently durable index-WAL participation remains a separate acceptance gate.
See [ADR 0027](adr/0027-double-ended-index-cursors.md).

Borrowed live primary rows now merge long keys, validate consumed tree keys and
physical images, and support arbitrary mixed end consumption. Long bounds retain
checked eligible point lookups. Historical clones, cold shared readers, 10000
wide rows, rollback/abort/reopen, both WAL versions and verified restore execute.
Imported projection verification streams the complete key set. See
[ADR 0028](adr/0028-borrowed-live-primary-rows.md).

SQL single-table primary ordering now consumes those borrowed rows in the requested
direction, applies the complete filter and stops after LIMIT matches. No-order
reads use the same projected-row path; joins/other orderings remain materialized.
Wide-table regression, independent integer/text models, actual script work/output
bounds, both-WAL restore, compiled CLI and real HTTP isolation/kill replay execute.
Explain keeps its existing primary_range/sorted contract. Wider query/load gates
and durable table-index WAL remain open. See [ADR 0029](adr/0029-streamed-primary-sql-order.md).

UPDATE/DELETE now borrow candidates and collect only the remaining transaction
event capacity. Early overflow preserves whole-script rollback; zero matches
still succeed at capacity. Key-only deletes, memory-capped reproduction/repair,
exact event boundaries, independent mutation/reopen/restore models and actual
CLI/HTTP kill replay execute. Snapshot staging remains bounded by its prior copy;
no durable format or production gate is closed. See
[ADR 0030](adr/0030-bounded-mutation-selection.md).

## Isolated registry and API keys

The synchronous server library publishes independent managed project directories,
persists checked version-1 metadata and hashes for 256-bit random keys, and rotates
credentials atomically. Single-use request capabilities preserve root ownership
and serialize same-project operations. Scope, traversal, symlink, permissions,
real capacity, concurrent request and metadata bounds execute. Passwords/sessions,
granular row authorization remain pending. Offline complete current-registry
backups now execute; future object/session data remains outside that archive.
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

Current live row-image locations now validate actual page/slot, event fingerprint,
table/key and persistent managed database identity. Old images retire on mutation;
rollback, reopen, both WAL versions and verified restore are covered. Existing
formats and 3072-byte table text keys are preserved. Atomic table/index WAL
participation remains pending; the following increment adds derived point routing. See
[ADR 0020](adr/0020-validated-live-row-locations.md).

Managed point lookup now derives and maintains the original B+ tree for integer
and short text keys; long text keys retain their existing path. Immutable caches,
failure/rollback preservation, 10000-row capacity, real incremental arena exhaustion
and SQL/restore behavior execute. Dense rebuilds preserve table admission. Persistent
index roots/pages/WAL and secondary indexes remain pending. See
[ADR 0021](adr/0021-derived-primary-key-trees.md).

Integer primary range plans now route necessary AND inequalities through linked
leaves for SELECT/UPDATE/DELETE. Endpoint/overflow, nullable/filter/alias, generated
read/write/reopen models and 6000-row narrow-work checks execute. Explain/client
contracts include primary_range. Secondary DDL
and independently durable index pages remain open. See
[ADR 0022](adr/0022-integer-primary-range-plans.md).

Text primary ranges now support necessary AND inequalities, UTF-8 byte ordering
and exact representable successor bounds. Long keys merge before LIMIT; longer
SQL bounds retain scans. Independent generated read/write models, 10000-key
capacity, nullable/reversed/NUL/Unicode edges, 6000-row narrow writes, both-WAL
restore and real CLI/container HTTP cases execute. No durable format or wider
production gate is closed. See [ADR 0026](adr/0026-utf8-primary-range-plans.md).

Explicit EBTI primary-tree images bind persistent database/table identity,
acknowledged transaction and exact relational history. Full live-key/pointer
verification precedes derived-cell installation. Foreign, stale and structurally
valid forged projections are rejected; capacity, generated mutation/replay and
both-WAL verified restore checks execute. Explicit private filesystem publication
and load now pass process-kill/sync/path/concurrent-writer/CLI/restore checks.
Bounded startup adoption is now implemented after mandatory WAL replay; durable table/index
WAL allocation. See [image format](table-index-image-format.md) and
[ADR 0023](adr/0023-bound-primary-tree-images.md).

Optional primary-ID.table-index files publish under existing database ownership
with descriptor-relative staging/rename/cleanup and private bounded reads. Eight
kill boundaries, eight sync-failure cases, two competing processes, generated
save/load/reopen histories and actual compiled container commands execute.
Cache errors leave the relational journal usable; backup omits these files.
Orphan cleanup, automatic refresh and table/index WAL remain open. See
[ADR 0024](adr/0024-private-primary-cache-files.md).

Startup adoption tries only current table-ID images with a 16-MiB total input
budget, full managed/liveness checks and count-only reporting. Missing/rejected/
skipped images reconstruct from WAL; valid indexes cannot mask WAL loss/damage.
Exact page-history digests are shared across immutable snapshots and reset on
accepted events. Capacity, bounded growth/truncation, cold/warm forks, actual
ACK-before-refresh kills, HTTP project isolation and both-version restore execute.
Automatic refresh/housekeeping, durable index WAL and production gates stay open.
See [ADR 0025](adr/0025-bounded-primary-cache-startup.md).

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

## Live directory-identity repair

Failing regressions reproduced a moved root redirecting project creation and an
accepted capability following a replaced directory. Controllers/capabilities now
pin root/project/data handles and check private modes/device/inode identity during
synchronous work. Authorization remains free of filesystem operations. Actual HTTP
tests verify generic refusal, preserved old/replacement data, healthy siblings and
explicit reopen. Formats are unchanged; broader hostile-admin filesystem/audit
and production gates remain open. See [ADR 0019](adr/0019-pin-project-directory-identities.md).

## Follow-up: complete standalone image envelope

[EBIP-1](image-plan-format.md) now accounts for complete changed physical components
including addresses, roots, retirements and its outer digest, bounded to9770208
bytes. Nested preflight precedes owned image-vector construction; complete exact
model replay remains mandatory. This is serialized length admission only. Numeric
heap/lifetime/worker reservation and the combined durable writer stay open under
[ADR 0048](adr/0048-bounded-physical-image-envelope.md); old runtime formats stay
unchanged and no stage acceptance is marked complete.

## Follow-up: retained serialized payload reservation

The optional [EnvelopePool](image-buffer-admission.md) now atomically reserves
complete EBIP byte lengths and buffer slots before encode/copy, including admitted
clones. Admitted preparation can serialize without releasing writer/generation
leases or exposing raw state. Raw plans, decoded/retained model state, temporary
validation and whole-process/server/WAL admission remain outside this scope;
[ADR 0049](adr/0049-admitted-serialized-image-buffers.md) closes no durable gate.

## Follow-up: streamed standalone index verification

[Complete index admission](streamed-index-admission.md) now checks bounded map/page
identity, topology and physical round trips one page at a time. Fingerprints and
wire bytes stay frozen. Borrowed target validation and direct private-candidate
admission remove redundant complete images/trees in delta generation/application.
Isolated full-capacity allocation guards and independent corruption/state models
execute under [ADR 0050](adr/0050-streamed-index-snapshot-admission.md). Candidate
map/retained/transient/worker quotas and the shared durable writer remain open.

## Follow-up: shared immutable index arena

[Original index pages](shared-index-pages.md) now share immutable decoded bodies
across snapshots while keeping private page maps and changed-page publication.
Pointer/owner identity, last-owner release, stable reuse/dense remapping, atomic
capacity refusal, independent state histories and actual concurrent model replay
execute under [ADR0051](adr/0051-shared-immutable-index-pages.md). Frozen bytes and
durable ACKs stay unchanged. Numeric map/page/transient/worker admission, cheaper
validated primary export and the combined durable writer remain open.

## Follow-up: shared validated primary projection

[Primary export](shared-primary-export.md) now performs complete stable arena
admission through a private map of shared immutable pages, then verifies every
eligible live row/current pointer. Dense source policy, sparse IDs, original bytes
and long-key fallback remain intact under [ADR0052](adr/0052-validated-shared-primary-export.md).
Native warmed-cache allocation regression, owner identity, refusal/import parity,
generated histories, actual thread writers and automatic model rebuild/replay
execute. Decoded plan/model/cache/transient/worker reservation and the combined
durable writer still require separate work.

## Follow-up: admitted decoded physical image vectors

The optional [DecodedPlanPool](decoded-plan-admission.md) now reserves complete
typed vector payload and owner slots after borrowed structural preflight and before
owned decoding. Exact capacity checks, last shared-owner release, independent-copy
charges, corruption/base refusal, actual concurrent races and native diagnostic
guards execute under [ADR0053](adr/0053-admitted-decoded-image-vectors.md).
Model/cache/staging/transient/worker heap, encoded/raw caller copies and the combined
durable writer remain outside this admission and require separate work.


## Follow-up: admitted destination replay lifetimes

[Destination replay](admitted-model-replay.md) now reserves per-project/global
writer exclusion and its output generation before complete physical reconstruction.
Borrowed AdmittedPlan input keeps separate vector accounting. Non-clonable output
retains leases through exact-instance publication/discard and exposes only borrowed
state. Actual ownership, refusal, historical readers, generated sequences and
thread caps execute under [ADR 0054](adr/0054-admitted-physical-replay-generations.md).
This counts lifetimes, not model/cache/staging/transient heap or runtime workers;
the combined durable writer and production gates remain open.


## Follow-up: compact retained relational payload

[Retained record ownership](retained-record-capacity.md) now removes caller
spare String/Vec capacity after validation, before shared live state retains it.
Reproduced schema/row/native regressions, exact wire parity, old readers, generated
histories and WAL1/2 commit/rollback/recovery checks execute under
[ADR 0055](adr/0055-compact-retained-record-payloads.md).
Caller input peaks and model/cache/map/staging/replay-transient budgets remain
outside this retained-shape guarantee; durable-index and production gates stay open.


## Follow-up: compact retained index buffers

[Retained index ownership](retained-index-capacity.md) now removes caller
spare text/key/pointer/child-vector capacity before immutable page publication.
Reproduced constructor/insertion/native failures, both ID policies, old owners,
full capacity, format parity and standalone reopen checks execute under
[ADR 0056](adr/0056-compact-retained-index-buffers.md).
Input peaks and allocator/model/cache/staging/replay-transient budgets remain
outside this shape guarantee; the combined durable writer and production gates
remain open.


## Follow-up: primary-key inner join probes

[Eligible JOIN plans](primary-key-joins.md) now probe the original right
primary index from a borrowed left scan. Complete ON/WHERE, long-key physical
checks, ordering/limits and existing work/output bounds stay active. Necessary
equality under AND is supported; other conditions keep the original fallback.
EXPLAIN and the matching strict project SDK accept primary_join under
[ADR 0057](adr/0057-primary-key-join-probes.md). This enables no new stored
format, durable-index acknowledgement, chained/outer join or production gate.


## Follow-up: stream ordered primary joins

[Unique left-primary JOIN order](ordered-primary-joins.md) now uses the
original point/range/double-ended source cursor and stops after accepted LIMIT
matches. Streamed plans retain selected output fields while complete ON/WHERE
and shared work/output budgets stay active. Other orders retain full stable sort
and intermediate limits under [ADR 0058](adr/0058-streamed-primary-join-order.md).
Stored formats, durability acknowledgements and production gates are unchanged.


## Follow-up: bounded primary-join sorting

[Small sorted JOIN limits](limited-primary-join-sort.md) now retain only
the best LIMIT full candidates with stable tie/null/type ordering. Full ON/WHERE
and every source probe still execute; work, matched-row, retained-byte and output
bounds stay active under [ADR 0059](adr/0059-bounded-primary-join-sort.md).
Other access paths keep their original limits. Stored formats, durable-index
acknowledgement and production gates are unchanged.


## Follow-up: admit projected payload before copying

[Output admission](projected-output-admission.md) now checks the shared
script allowance before copying each selected row. Full sorting projects
incrementally; streamed paths use the same borrowed preflight. A reproduced
native refusal peak falls from 99388593 to 14210993 bytes under
[ADR0060](adr/0060-admit-projected-output-before-copy.md), with identical
errors/committed bytes. This is logical output accounting; whole-memory, durable
index and production gates remain open.


## Follow-up: borrowed ordinary table sorting

[Non-primary table orders](limited-table-sort.md) now borrow checked
source rows and retain only the best LIMIT candidates. Necessary points/ranges
restrict access, while complete WHERE and every candidate still execute. Stable
ties, all supported sort types, original work/match/retained/output bounds and
WAL 1/2 recovery hold under [ADR 0061](adr/0061-borrowed-bounded-table-sort.md).
The reproduced warmed 1500-row peak falls from 4842026 to 11634 requested bytes.
This operation-local observation closes no cold-memory, transient, combined
durable-writer or production gate. General fallback joins retain their limits.


## Follow-up: borrow before candidate admission

[Borrowed candidates](borrowed-query-candidates.md) let primary JOIN
predicates/projection read checked source slices directly. Sorted table/JOIN heaps
compare and charge winners before cloning; discarded candidates still consume
the original match/work counts. Reproduced warmed allocation totals nearly halve
under [ADR 0062](adr/0062-admit-borrowed-candidates-before-cloning.md).
Physical validation still allocates; these operation totals are not a process
quota. Stored formats, durability acknowledgements and production gates remain.


## Follow-up: borrowed physical row comparison

[Physical verification](physical-row-comparison.md) shares one validating
cell codec with original owned decode. Checked row resolution compares borrowed
payload with immutable live state, retaining identity/digest/full-format checks
under [ADR0063](adr/0063-compare-physical-rows-with-borrowed-codec.md).
Warmed1000-resolution requests fall from3344000/6416000 to104000 bytes; schema
validation still allocates. This closes no cold-memory, combined durable writer
or production gate and changes no stored format or acknowledgement.


## Follow-up: bounded stack schema validation

[Schema checks](stack-schema-validation.md) retain every original rule
and error order using bounded borrowed-name arrays. Oversized names do not enter
sort comparisons. Native schema/key and physical-resolution samples request no
temporary heap under [ADR0064](adr/0064-bounded-stack-schema-validation.md).
These observations exclude stacks/cold fixtures and close no numeric whole-memory,
combined durable writer or production gate. Stored bytes and ACK are unchanged.


## Follow-up: borrowed complete schema inventory

[Snapshot metadata](borrowed-schema-inventory.md) now supports borrowed,
exact-size iteration in live table-ID order. Model preparation and complete root
validation preserve every check while avoiding column copies; status reads a
count, and cache warmup retains only bounded table names. The 128-by-64 warmed
prepare sample falls from1,459,120 to22,192 requested bytes, peak723,160 to4,784.
A separate1000-pass inventory scan allocates zero. [ADR0065](adr/0065-borrowed-schema-inventory.md)
records ownership, ordering, evidence and limits. File/WAL/SQL/HTTP meanings stay
unchanged. Numeric model/cache/staging/transient budgets and the combined durable
writer remain open; this does not complete a production gate.


## Follow-up: borrowed nested-loop sources

[Fallback joins](borrowed-nested-join.md) now retain bounded checked row
references and evaluate complete ON/WHERE before any candidate copy. Full matched
row limits, stable sort/source ties, pair/predicate work and projection timing
remain unchanged. With100 rows per side and3000-byte hidden payload, false ON/
WHERE summed requests fall64,469,242/64,468,894→9530/9182. [ADR0066](adr/0066-borrowed-nested-loop-sources.md)
records evidence and limits. The planner retains bounded_nested_loop; this adds
no fallback TopK or public point/range extraction. Numeric model/cache/staging/
transient budgets, combined durable writer and production gates remain open.

## Updated source planning estimate, 2026-10-08

The current implementation and tests exceed74 thousand source lines while major
platform modules remain open. Revise the original80–160 thousand planning range
to roughly110–180 thousand total, including tests: approximately35–105 thousand
additional lines at the current size. Dashboard, private objects, realtime, roles/
row policies, Kotlin SDK, coordinated backups, durable engine/resource gates and
load/security/upgrade checks drive that remaining scope. This is a rough planning
range, not a code-volume target or completion promise; do not pad implementations.


## Follow-up: explicit native private-root mode

[ADR0082](adr/0082-native-private-root-mode.md) selects a previously verified root
through EMILYBASE_ACCOUNT_ROOT, mutually exclusive with an explicit legacy data
directory. Invalid configuration refuses before filesystem work; existing roots
open off the reactor without bootstrap/reset. Actual HTTP ACK/forced-kill/restart,
single-winner refresh, key rotation, independent restored-clone authority and
corrupt-private-WAL startup refusal execute on WAL1/2. This closes the earlier
native-mode follow-up; public signup/RLS, dynamic roster, container adapter,
whole-process resource admission and production acceptance remain open.


## Follow-up: explicit first private root

[ADR0083](adr/0083-explicit-private-root-initialization.md) adds the offline first-root
CLI: one new project/private v3 store, trusted initial time, no input archive or
printed key/password. Operator list/key rotation and service-key provisioning work
with the actual binary. Complete captured histories and final retained owners gate
publication. A reproduced late private-container substitution now refuses in both
initialization and restore. Eight initialization cases cover generated empty models,
eight sync faults, six native kills, eight final substitutions and two competing
processes; CLI and real HTTP checks remain separate evidence. Public signup/RLS,
dynamic roster, resource/container and production gates remain open.


## Follow-up: private HTTP credential management

[ADR0084](adr/0084-private-http-credential-management.md) exposes current-password
change and trusted disable/enable under the existing current project service key.
Durable epoch changes revoke old access/refresh families; re-enable never revives
them, while same-state disable requests preserve epoch/history. Scoped input and
native ACK-kill/restart cases cover WAL1/2. Public password reset, user roles/RLS,
family cleanup and production gates remain open.


## Follow-up: separate private-root container adapter

[ADR0085](adr/0085-explicit-private-root-container.md) keeps the default registry
image behavior and adds an independent accounts target/Compose file. Explicit
first-root initialization and offline common backup/restore remain operator
operations. The same synthetic lifecycle has a native preflight and an actual
Docker mode; five received-ACK kills, single-winner refresh, credential epochs,
WAL2 clone/source authority, logout and corrupt private WAL refusal execute.
Native stable/minimum checks passed; first new hosted container execution is
pending. No stage, resource, security or production gate is declared complete.


## Follow-up: explicit bounded inactive-session cleanup

[ADR0086](adr/0086-bounded-private-session-cleanup.md) exposes the existing deletion
transaction through a retained current-service-key gate and strict private HTTP.
One request removes at most128 inactive families, preserving refreshable authority;
clock observation is separately durable and WAL byte reclamation remains offline.
Invalid limits/client time refuse before private clock work. Scoped history/input
checks, eight generated count models, a real TCP cleanup race and ACK-kill/reopen
on WAL1/2 cover this boundary. The initial private-root container lifecycle passed
on7801d74; the expanded cleanup scenario requires its next hosted run. Automatic
scheduling, public account policy, roles/RLS and production acceptance remain open.


## Follow-up: bounded private user metadata pages

[ADR0087](adr/0087-bounded-private-user-pages.md) adds synchronous/current-service-key
listing and strict HTTP pages of at most128 users. Exclusive canonical-login
continuation reads current state without a cross-request snapshot; no private
verifier/session export, KDF, clock observation or WAL commit occurs. Compatibility,
130-user boundaries, corruption, eight generated models, exact history, waiting-body
key rotation and real TCP restart cases cover this boundary. The expanded cleanup
container lifecycle passed on90bba0b; listing needs its own next hosted run. Public
account policy, roles/RLS, whole-process admission and production acceptance remain open.


## Follow-up: private master-key files

[ADR0088](adr/0088-private-master-key-file.md) adds one-source bounded startup
configuration, exact private file validation and controlled-restart key replacement.
A standalone private-root Compose variant receives only a path; explicit one-off
stdin provisioning keeps secrets out of arguments and server ENV. Original
environment deployments remain available. Files are plaintext and excluded from
root bundles; encryption, runtime reload, parent sandboxing and production
acceptance are not completed by this adapter. Native refusal/restart and the common
environment/file lifecycle provide the corresponding local verification boundary.


## Follow-up: bounded logical table exchange

[ADR0089](adr/0089-bounded-logical-table-transfer.md) adds original-engine table
export/import through CLI streams. Strict versioned JSON preserves typed schema,
primary order and exact finite float bits. Complete export refuses above255 rows;
new-table import is one durable transaction with no merge or overwrite. Private
backup/session restore, large continuation protocols, foreign converters, public
authorization and production acceptance remain separate open work.


## Follow-up: project HTTP logical exchange

[ADR0090](adr/0090-project-scoped-http-table-transfer.md) connects the verified
logical format to both service routers. Existing authorization/admission applies;
HTTP input/export cap65,536 bytes, no truncation, no-cache responses and one durable
new-table commit. Private-root mode rechecks current keys after body waits without
private-clock/WAL writes. Native/real-TCP and seven-kill common lifecycle checks
cover this boundary. Large transfer, user policy/roles/RLS, global resource gates
and production acceptance remain open.


## Follow-up: project table schemas over HTTP

[ADR0091](adr/0091-project-table-schema-api.md) adds bounded inventory, schema
inspection and typed create/drop through both service routers. Original catalog
and WAL transactions preserve IDs, atomic deletion and empty recreation. Current
private-root keys are rechecked after body waits; private history/time is unchanged.
Generated catalog sequences, maximum inventory, scoped refusals and native create/
drop ACK kills cover this adapter; common lifecycle now has nine kills. Public user
policy, migrations, resource gates and production acceptance remain open.


## Follow-up: bounded typed row HTTP

[ADR0092](adr/0092-bounded-project-row-api.md) exposes original-engine get/page and
single-row insert/update/delete in both service routers. Exact numeric wire,
checked primary cursors, exclusive current-state continuation, bounded output and
single durable mutation preserve the engine boundary. Private/sibling histories,
user/current-key authority, generated models, ASan parsing and real TCP CRUD ACK
kills cover the adapter. Common lifecycle now has twelve kills. Batch/idempotency,
public account policy, RLS, global admission and production acceptance remain open.


## Follow-up: atomic service row batches

[ADR0093](adr/0093-atomic-service-row-batches.md) adds bounded ordered row writes on
one table in one original transaction. Complete wire preparation precedes staging;
late schema/existence failures abort all earlier changes.256/257 bounds, generated
independent models, exact history, concurrency and native before-commit/after-ACK
kills cover the adapter. Common lifecycle has thirteen successful ACK kills.
Idempotency, held network transactions, public user/RLS authority, whole-process
budgets, stable format and production acceptance remain open.


## Follow-up: exact typed row SDK

[ADR0094](adr/0094-exact-row-sdk-transport.md) connects six existing Rust row routes
to the scoped SDK. New exact decimal/bit types, bounded copied requests and strict
response validation preserve full numeric ranges without changing SQL's numeric
wire. Mocked protocol regressions and real stable/minimum TCP ACK-kill cases cover
this client adapter. No npm release, browser/public user authority, RLS, Kotlin or
production acceptance is completed.


## Follow-up: owned staged SQL for composed transactions

[ADR0095](adr/0095-owned-staged-sql.md) introduces an owning synchronous query API
that combines typed prefix/SQL/suffix writes before one original WAL commit. Every
validation/parse/plan/run error consumes and discards the transaction; successful
results require explicit commit or discard. Existing execute/control and format
contracts remain intact. Generated committed models, exact history, shared event/
query budgets, backup/restore and before/after-ACK process kills verify this boundary.
Migration receipt/version policy, HTTP exposure, global resource gates and
production acceptance remain open.


## Follow-up: bounded offline project migrations

[ADR0096](adr/0096-atomic-bounded-migrations.md) adds a synchronous migration crate
and explicit CLI. Consecutive versions1..128 bind exact SQL/label bytes; the first
ledger, script and receipt share one original commit. Identical historical retries
are no-ops; changed/skipped/failed/corrupt metadata refuses without repair. Metadata
consumes normal event/table/row limits and survives verified restore. Controlled
first/next staged/ACK kills, a separate generated version model, digest oracle and
actual CLI refusals cover this boundary. Owner-writable receipts are not independent
audit authority. Online orchestration/schema diff/ALTER/down, global resource gates
and production acceptance remain open.


## Follow-up: bounded INSERT SELECT and migration rebuilds

[ADR0097](adr/0097-bounded-insert-select.md) extends the original AST/parser/executor
with typed column-copy writes using existing SELECT plans. Resolve before scanning,
retain at most event-capacity+one, finish reading before self-inserts and refuse
overflow/late conflicts atomically. Migrations can rebuild small schemas with a
receipt in the same commit;125/126-row boundaries explicitly count that receipt.
Independent copy models, exact history, native staged/ACK recovery, actual CLI/TCP
and sanitizer parser/mutation campaigns cover the boundary. ALTER/schema diff/down,
large/online changes, full upgrade/resource/security and production gates stay open.


## Follow-up: project-service migration HTTP

[ADR0098](adr/0098-project-migration-http.md) exposes the same bounded migration
contract through both authorized project routers. Strict metadata, retained data
gates, current-key checks in private-root mode and conservative outcome errors
preserve original atomicity. Exact concurrent retries commit once; receipts are
included in ordinary compact/backup/restore. Native TCP ACK kills and the common
root lifecycle cover recovery and private-history isolation. Online/schema-diff
changes, RLS, wider upgrade/security/resource and production gates remain open.


## Follow-up: exact migration SDK

[ADR0099](adr/0099-exact-migration-sdk.md) adds strict project-service migration
methods to the local TypeScript client, without moving engine logic from Rust.
Bounded immutable definitions and exact receipt strings preserve retry identity;
transport uncertainty never triggers an automatic second write. Native original
server checks include a separate digest oracle, schema copy, concurrent retries,
ACK/lost-response restarts and current-key/sibling denial on both Rust versions.
Online/schema-diff changes, end-user policy/RLS and production gates remain open.


## Follow-up: bounded row policy decisions

[ADR0100](adr/0100-bounded-row-policy-decisions.md) supplies a synchronous pure
Rust decision model with explicit per-operation rules, shared node/depth/literal
bounds, actual borrowed private principals, complete table identity and old/new
UPDATE checks. Exact-history, independent decision models and original transaction
rollback/recreation tests cover this library. Durable policy installation/revisions,
roles, query filtering and atomic authenticated user CRUD/HTTP remain open. Existing
service authority is unchanged; stage5 and RLS acceptance are not complete.


## Follow-up: bounded original policy records

[ADR0101](adr/0101-bounded-original-policy-records.md) encodes exact policy/schema
bytes as one metadata row plus at most seven original typed fragments, preserving
existing engine limits. Complete identity/checksum/order/length validation precedes
nested schema/policy compilation. Atomic replacement rollback and original verified
backup/restore compose in tests on both WALs. Actual private catalog versioning,
install/CAS/inventory/revisions, roles and user-data enforcement remain open.


[ADR0102](adr/0102-explicit-private-policy-catalog.md) adds an explicit private v4
catalog using original typed records and actual private commit revisions. Atomic
migration/install/replacement, exact expected-revision retries, complete inventory
validation and current borrowed policy proofs do not enable a user data route.
Private/common-root restore preserves policies while resetting session incarnation.
Roles, trusted public transaction context and bounded filtered CRUD remain open;
no platform stage closes. See the [catalog contract](policy-catalog.md).


[ADR0103](adr/0103-service-key-policy-administration.md) connects explicit policy
administration to current project service keys and the actual held public table
context. Root-only enable/list/install HTTP preserves exact u64 receipts, CAS
retries and private/public/sibling histories. No user SQL/rows/roles or implicit
migration is enabled. See [policy administration](policy-administration.md).


## Follow-up: owned user-row enforcement

[ADR0104](adr/0104-owned-user-row-policy-enforcement.md) applies current installed
policies to exact-key reads or 1..256 typed writes while both public/private owners
remain held. Staged old/new checks and one original commit preserve packet atomicity;
SELECT-denied and absent reads both return no row. Current credentials, policy,
table identity/schema, clock and restored incarnation are rechecked per call.
Generated row models, serialized competing writers and controlled staged/received
result kills cover both original WALs. This synchronous trusted gateway requires
a service key and user access token; it exposes no public user HTTP or SQL.
Filtered pagination, roles, public admission and broader production gates remain
open. See [the precise contract](user-row-enforcement.md).


## Follow-up: current-policy filtered native pages

[ADR0105](adr/0105-current-policy-filtered-keyset-pages.md) adds current-state
primary-order pages of 1..128 permitted rows to the owned synchronous gateway.
Hidden gaps/tails return neither rows nor scan watermarks; a continuation contains
only the last returned visible key when another permitted row exists. Deleted
cursor keys, current policy/session changes, exact large integers and long text
bounds preserve the original key ordering. Independent owner-map models, full-source
hidden scans, large row payloads, reopen and verified restore cover this boundary.
No user HTTP, multi-request snapshot, signed capability, roles or new parser is
introduced. See [page semantics](user-row-pages.md). Broader stage-5 and production
gates remain open.


## Follow-up: trusted backend policy-enforced row HTTP

[ADR0106](adr/0106-trusted-backend-user-row-http.md) adds root-only get/page/write
routes requiring the current service Bearer key and a separate current user access
header. The original strict lossless row decoder maps to the owned policy gateway;
current key/private roster are rechecked after body waits before decoding. Current
policy/session changes during those waits apply before public work. Complete output
remains bounded, and ambiguous responses require inspection. Typed key validation
now distinguishes bad client input from physical lookup failure.
Router/transport/owned-core regression, actual TCP received/unread-result kills,
verified root clone, strict stable/minimum checks, OpenAPI and sanitizer fuzzing
cover the adapter. No legacy route, browser service key, user-only admission,
public signup, roles or automatic retry is added. See [HTTP contract](user-row-http.md).
Stage5 and broader security/load/upgrade/resource/production gates remain open.


## Follow-up: offline current-key policy CLI

[ADR0107](adr/0107-offline-service-policy-cli.md) adds explicit local enable/list/
install commands for an existing stopped private root. Current service keys come
from the original bounded private file loader; definitions use exact bounded stdin
and receipts preserve full decimal u64 strings. Actual table identity/schema,
exclusive owners, original commits and exact retry rules stay in the native root.
Input waits hold no data owner; current rotation after a wait refuses the copied
key. See [operator contract](policy-cli.md). This does not close public admission,
roles, security/load/upgrade/resources or production gates.


## Follow-up: offline private user administration

[ADR0108](adr/0108-offline-private-user-cli.md) adds trusted create/list/disable/
enable commands to an existing stopped root. Shared private-file service keys and
bounded redirected password input keep secrets out of arguments/output; a real
terminal refuses before echo. Original KDF, current-key, exact-login pagination,
epoch revocation/no-op, exclusive ownership and verified restore semantics apply.
Concurrent same-login provisioning creates one account and never replaces its
password. No session is issued and no trusted clock/private version is reset.
See [operator contract](user-cli.md). Public user admission, roles and production
security/load/upgrade/resource gates remain open.


## Follow-up: durable offline service-key publication

[ADR0109](adr/0109-durable-offline-service-key-file.md) adds offline project metadata
and private key-file rotation. The original owned staging mechanism publishes and
syncs a fresh external0600 secret before activating its original registry digest;
existing/internal/aliased targets refuse. The complete bootstrap can provision
users offline without secret terminal output. Ordered publications explicitly
permit an inactive file or an already-active uncertain result after a crash; there
is no cross-filesystem transaction or automatic retry. Native injected failures,
substitution/process-kill matrices, CLI output-write failure, verified clone,
existing rotations and private HTTP checks cover the boundary. See
[operator contract](offline-service-keys.md). No format or production gate closes.


## Follow-up: explicit closed public admission metadata

[ADR0110](adr/0110-explicit-closed-public-admission-catalog.md) introduces explicit
v4-to-v5 migration with a closed singleton, exact current-revision CAS and readonly
identical retries. Complete private/root validation preserves legacy formats.
Verified restore closes an enabled copy in the same commit that resets session
incarnation/time while preserving rows, policies and the unchanged source.
This is native operator metadata; public user-only gateways/routes, signup, roles,
SDK/dashboard integration and broader production gates remain open. See
[the catalog contract](public-admission-catalog.md).


## Follow-up: native admitted user gateway

[ADR0111](adr/0111-native-admitted-user-gateway.md) adds synchronous current-user
auth/session/metadata and owned typed row operations without project service keys.
Missing/closed v5 denies before password/time/public work. Current private proof,
policy and real data ownership remain held through original commits. Intentional
reopen can resume a current session; verified clone still requires fresh login.
Generated owner/admission models, concurrent users and process kills cover the
native boundary. See [the contract](native-public-user-gateway.md). Public HTTP,
signup, roles, user SDK/dashboard and production acceptance remain open.


## Follow-up: explicit offline public admission

[ADR0112](adr/0112-offline-public-admission-cli.md) adds current-private-key
enable-catalog/status/open/close commands for a stopped existing root. Migration
remains explicit and closed, revisions retain exact unsigned decimal strings,
original CAS prevents stale reopen, and stdout failure requires inspection after
the original durable operation. Both WALs, current users, independent generated
histories and nonempty verified clone are covered by actual binary scenarios.
See [operator contract](admission-cli.md). This does not enable public HTTP,
signup/roles, user SDK/dashboard or production acceptance.


## Follow-up: admitted public user session HTTP

[ADR0113](adr/0113-admitted-public-session-http.md) adds separate account-root
user sign-in/refresh/logout/me routes without service credentials. Current closed
admission refuses before body work and again before the server clock after waits;
native session/filesystem checks retain actual ownership. Shared original worker,
project and socket-peer budgets, strict object JSON/header handling and no-store
static logs preserve the boundary. Positional Serde arrays in original private
account requests and clock-before-closed-refusal are reproduced before correction.
Actual TCP received session results, forced stops and nonempty verified copy cover
the new adapter. See [HTTP contract](public-session-http.md). Public user row HTTP,
signup/roles, user SDK/dashboard and production acceptance remain open.


## Follow-up: admitted public typed row HTTP

[ADR0114](adr/0114-admitted-public-user-row-http.md) adds user-only get/page/write
routes under the original current admission/session/table-policy owners. Original
bounded lossless requests and complete packet commits are reused; service keys,
user SQL/admin and migration-ledger access remain excluded. New router/native TCP
checks cover hidden reads/pages, late policy rollback, delayed current state,
signed wire/bounds, concurrent uniqueness, received/unread results and verified
nonempty copy. A positional-array defect is reproduced in both original row
grammar validators before adding the shared object-only guard. See
[HTTP contract](public-row-http.md). Signup/roles, user SDK/dashboard, browser
integration and production acceptance remain open.


## Follow-up: explicit user TypeScript client

[ADR0115](adr/0115-explicit-user-typescript-client.md) separates admitted user
session/row transport from privileged service-key SQL/migration authority. The
client takes caller-owned purpose tokens, snapshots bounded typed packets and
preserves exact integer metadata without credential storage or automatic retries.
Unit regressions cover missing-token dispatch, mutable receipt-count input and
invalid options. Actual Node Fetch/Rust binary checks on both toolchains and WALs
cover owned reads/pages, late packet rollback, received-result kills, deliberate
lost responses, nonempty verified restore and offline revocation. See
[user contract](user-sdk.md). Signup/roles, browser/device integration, persistent
session orchestration, dashboard/Kotlin and production acceptance remain open.


## Follow-up: admitted own-password change

[ADR0116](adr/0116-admitted-own-password-change.md) adds an access-and-old-password
operation deriving the exact account inside the retained private owner, then using
the original durable digest/epoch replacement. All older families are revoked;
no token pair, identity selector or administrative reset is exposed. HTTP shares
current admission/body/worker/rate/clock boundaries and the user SDK transports the
same explicit method. Both WALs, generated raw-password epoch histories, competing
changes/refreshes, delayed state, actual received/unread TCP kills and verified copy
are checked separately. See [contract](user-password-change.md). Signup/roles,
recovery channels, browser/device policy and production acceptance remain open.


## Follow-up: original native object-file foundation

[ADR0117](adr/0117-scoped-object-files.md) introduces a bounded scoped binary
image with payload SHA-256/header CRC32 and full expected-identity validation.
Native no-replace publication reuses the original retained private stage and sync
contract; readonly offline inspection reports metadata only. Database/WAL and
root backup formats remain unchanged. See [format](object-format.md).
HTTP upload/download, authenticated project inventory/policies, quotas, signed
URLs, cleanup/delete, object backup integration and production gates remain open.


## Follow-up: retained native project object directories

[ADR0118](adr/0118-retained-project-object-directories.md) binds typed object IDs
to an exclusively owned private directory and retained exact project marker.
Descriptor-based read/publication continues in the original inode after namespace
moves; marker replacement/current admission refuses. Offline CLI supports binary
put and metadata inspection without secret payload output. A deterministic
inherited-description regression precedes the explicit lock-release fix. See
[contract](object-directories.md). This remains outside AccountRoot, public HTTP,
user file policies, quotas, inventory/delete and verified root backups.


## Follow-up: complete bounded object inventory and capture

[ADR0119](adr/0119-bounded-object-inventory-capture.md) verifies every object in a
bounded retained directory, refuses unknown entries and returns sorted metadata
with a scoped deterministic digest. Immutable capture copies matching bytes and
rechecks the complete source for a future archive encoder. Offline list prints
metadata only. See [contract](object-inventory.md). These are work bounds, not put
quotas. Persistent object archive/restore and root backup integration remain open.


## Follow-up: complete checked object archive byte format

[ADR0120](adr/0120-checked-object-archive-format.md) frames immutable captured
objects with complete scoped count/length/digest validation and nested envelope
checks. It supports canonical encoding/borrowed verification and a bounded readonly
metadata CLI. See [format](object-archive-format.md). Encoding bytes is not durable
backup publication or restore. Those gates and AccountRoot integration remain open.

## Follow-up: owned native object archive publication

[ADR0121](adr/0121-owned-native-object-backup.md) retains the actual destination
parent before complete capture, excludes the source inode, and reuses the original
owned private byte publisher. A retained selected-file result permits final full
archive/inode/report verification without reopening a substituted name. The native
CLI publishes metadata only; uncertain or lost results require explicit inspection.
See [contract](object-backup.md). Verified object restore and coordinated AccountRoot
backup integration remain open; no platform or production milestone is closed.

## Follow-up: isolate token allocation diagnostics

[ADR0122](adr/0122-isolated-token-allocation-sample.md) moves the existing token
measurement into a separate opt-in native process after a hosted allocation
failure and controlled reproduction of unrelated-process attribution. Exact
counters and a failing144-byte negative control are preserved. This changes the
diagnostic boundary, not token encoding/auth semantics or production acceptance.

## Follow-up: verified native object archive restore

[ADR0123](adr/0123-verified-native-object-restore.md) verifies the entire scoped
archive before staging and reconstructs all canonical files through an original
storage-level private directory publisher. Complete inventory/input rechecks,
directory/parent fsync and no-replace selection precede a metadata result. Selected
identity and complete inventory are checked again. Nonempty unselected private
stages remain for explicit inspection; there is no recursive janitor or overwrite
retry. See [contract](object-restore.md). This closes the standalone native copy
operation only; AccountRoot coordination and platform/production gates remain open.

## Follow-up: explicit native object write limits

[ADR0124](adr/0124-explicit-native-object-write-limits.md) admits an immutable fresh
object against a complete current inventory and explicit per-call count/byte
limits. Expected final sorted metadata/digest is prepared before staging; complete
source/result rechecks preserve uncertain outcomes. The CLI bounds input before
locking and prints metadata only. See [contract](object-write-limits.md). This is
not persisted quota policy, authorization, a service resource gate or an HTTP
upload endpoint; original native put remains explicitly separate.


## Follow-up: deterministic CLI inventory output regression

The actual128-object CLI list now runs through a forced4096-byte pipe. Its test
helper drains bounded stdout/stderr while the child runs and closes the retained
parent writer, avoiding both pipe-capacity deadlock and missing EOF. The old
wait-before-read order fails the controlled deadline; the corrected case passes
20 fresh runs on each supported toolchain. This closes a test-harness defect only;
no database format, file-service or production milestone changes.


## Follow-up: retain selected object identity through final receipts

[ADR0125](adr/0125-retained-object-publication-identity.md) extends the original
retained-file publisher to trusted paths and uses actual selected descriptors for
standalone objects, project markers and native put. Bounded put keeps that identity
through its later inventory/receipt boundary. Four identical-content replacement
regressions precede their corrections. Observed changes preserve artifacts and
require inspection. Formats and platform/production acceptance remain unchanged.


## Follow-up: bounded streaming object metadata

[ADR0126](adr/0126-streaming-object-metadata-verification.md) shares exact original
v1 header admission between whole-byte and bounded-reader verification. Complete
native inspection/inventory stream payload hashing with8192-byte scratch, keeping
retained inode/private metadata/current scope checks. Owned get/capture/publication
and archive formats remain unchanged. This is not a network interface, numeric
whole-process reservation, persistent quota or production acceptance.

## Proposed next boundary: files in the retained account root

[ADR0127](adr/0127-proposed-account-root-object-integration.md) is proposed only.
It defines a blob-first/private-catalog visibility protocol and requires actual
selected-file ownership across the catalog commit, orphan quota accounting,
current file authority and complete coordinated backup/restore before user HTTP.
The exact schema/version, lock order, quota, idempotency and reclamation contracts
remain open. Current manifests, bundles and endpoints are unchanged. The current
[architecture diagram](architecture.md) separates implemented user/service paths
from standalone operator object tools; no platform stage is marked complete.

## Follow-up: native streamed single-object inspection

[ADR0128](adr/0128-native-streamed-object-inspection.md) adds report-only native
inspection and routes the existing CLI through complete bounded streamed hashing.
Selected descriptor ownership survives the final marker/metadata/inode checks.
Actual readonly maximum/late-corruption CLI and native mutation/model tests pass
on both toolchains. See [contract and local memory observations](native-object-inspection.md).
This exact-object operation does not certify complete inventory or add file HTTP,
persisted quotas, root integration, stored versions or production acceptance.

## Follow-up: bounded seekable object archive metadata

[ADR0129](adr/0129-bounded-seekable-object-archive-inspection.md) preserves original
outer-checksum priority with a complete body hash/EOF pass followed by bounded
nested-object/frame/inventory validation. Native inspection and selected backup
readback retain file checks without owning a full archive image. Capture/encoding/
restore, format versions and durability points remain unchanged. See
[contract](seekable-object-archive-inspection.md). This is not a network upload,
whole-process resource quota or coordinated AccountRoot/file service.

## Follow-up: borrowed canonical object archive encoding

[ADR0130](adr/0130-borrowed-canonical-object-archive-reader.md) adds a synchronous
Read+Seek view over immutable verified snapshot/archive images. Small bounded
frames and a shared canonical header replace a second complete payload image in
this API; caller buffers receive exact original bytes. Owned encoding and native
backup publication retain their existing behavior until a separate durable reader
publisher passes complete readback/selection/fault tests. See
[contract](borrowed-object-archive-encoding.md). No platform stage closes here.

## Follow-up: bounded immutable-reader publication

[ADR0131](adr/0131-bounded-reader-publication-and-object-backup.md) connects the
immutable archive reader to a separate synchronous bounded publisher with exact
EOF, full byte readback, stable private staging, retained selected descriptor and
original no-replace/fsync/uncertain outcomes. Native backup avoids a second full
encoded payload image while preserving source/destination/final archive checks.
Owned live capture and standalone restore remain. See
[contract](reader-file-publication.md); AccountRoot/files and production gates stay open.

## Follow-up: selected native object retention

[ADR0132](adr/0132-selected-native-object-owner-retention.md) adds opaque selected
objects/bounded receipts borrowing their original native directory owner and keeping
the actual published descriptor across later caller work. Full streamed revalidation
refuses byte/scope/marker/private-metadata and identical-byte inode substitutions.
Original write/durability paths remain. This supplies one native prerequisite for
the still-proposed AccountRoot protocol, not current user authority, catalog commit,
persisted quota or coordinated backup. See [contract](selected-object-retention.md).

## Follow-up: explicit Pager lock owner lifetime

[ADR0133](adr/0133-explicit-pager-file-lock-owner.md) fixes a deterministic inherited-
description lifetime regression with a private guard retained from successful lock
acquisition through constructor errors or the returned Pager. Active exclusion,
successor ownership, byte formats, fsync/unknown outcomes and poisoned-write refusal
remain. See [contract](pager-owner-lock-lifetime.md). This does not alter other owner
implementations or close broad concurrency/recovery/production acceptance.

## Follow-up: retained bounded native payload reads

[ADR0134](adr/0134-retained-bounded-object-payload-reader.md) adds an opaque fully
admitted object cursor borrowing its original directory owner and retaining the
actual readonly inode. Reads copy at most8192 payload-only bytes with before/after
scope/metadata/identity checks; failed checks clear the attempted prefix and poison
the handle. Checked payload seeks, full revalidation and consuming finish preserve
the original scope/report. See [contract](object-payload-reader.md). This adds no
HTTP authority, persistent file catalog/quota or coordinated root restore gate.

## Follow-up: standalone original-WAL file references

[ADR0135](adr/0135-original-wal-native-file-reference-catalog.md) adds a native
operator FileStore owning separate original-engine metadata and scoped objects.
Its exact private schema persists physical quotas and logical references with
scope/hash/commit revision. Blob-first publication retains the actual selected inode
through the own-WAL commit and final complete receipt checks. Valid unreferenced
objects stay invisible but charged; uncertain outcomes require reopen/inspection.
See [contract](native-file-catalog.md). Metadata mutations/deletion and coordinated
backup remain pending; no user file route, Root version or platform stage closes.
