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
| 5 | auth, policies, objects, realtime, dashboard, SDKs, backups | access tests, token rotation, verified restore | backup foundation, TypeScript SDK, password helper and local private account store implemented; local durable sessions and coordinated offline root backup/restore implemented; explicit native account/session mode tested on real TCP with WAL1/2; public account policy remains open |
| 6 | deployment, upgrades, load/security audit, converter | recovery/backup/upgrade matrix passes | experimental Docker/Compose tested; format upgrade/load/security gates open |

## Not supported

Durable table indexes, background journal rotation and history vacuuming,
Extended SQL, public signup, row policies,
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
