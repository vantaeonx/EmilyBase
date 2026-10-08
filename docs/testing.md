# Verification

Run `cargo test --locked --workspace` for unit, file integration, subprocess CLI
and property tests. Each property runs 256 generated cases by default. File tests
use isolated temporary directories and synthetic payloads. Disk-backed properties
override the default with 32 or 64 cases. They exercise reopen,
no-clobber creation, concurrent creation, advisory locks, truncation, corruption
and record mutation. They do not prove power-loss safety or transactional recovery.

## Format fuzzing

The engine builds on stable. Optional coverage-guided fuzzing uses nightly and
`cargo-fuzz` with AddressSanitizer:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
cargo +nightly fuzz run file_format -- -max_total_time=30 -max_len=4096 -rss_limit_mb=512
cargo +nightly fuzz run catalog_records -- -max_total_time=30 -max_len=4096 -rss_limit_mb=512
cargo +nightly fuzz run wal_records -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
cargo +nightly fuzz run owned_journal -- -max_total_time=45 -max_len=20000 -rss_limit_mb=512
cargo +nightly fuzz run commit_metadata -- -max_total_time=45 -max_len=4096 -rss_limit_mb=512
cargo +nightly fuzz run commit_model -- -max_total_time=45 -max_len=256 -rss_limit_mb=512
cargo +nightly fuzz run profile_report -- -max_total_time=45 -max_len=8192 -rss_limit_mb=512
cargo +nightly fuzz run managed_recovery -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
cargo +nightly fuzz run row_locations -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
cargo +nightly fuzz run primary_lookup -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
cargo +nightly fuzz run text_ranges -- -max_total_time=60 -max_len=4096 -rss_limit_mb=512
cargo +nightly fuzz run sql_mutations -- -max_total_time=30 -max_len=4096 -rss_limit_mb=512
cargo +nightly fuzz run backup_archive -- -max_total_time=30 -max_len=32768 -rss_limit_mb=512
```

The target checks raw headers, pages and pages with a repaired checksum to reach
structural validation. Add valid synthetic header/page seeds to
`fuzz/corpus/file_format` for better coverage. Corpus and crash artifacts are
ignored. A bounded smoke run is not a complete fuzz campaign or a security audit.
The catalog target checks schema, row and relational-event codecs and round trips
accepted records. Synthetic `ESCH`, `EROW` and `ETBL` seeds improve its coverage.
The backup target checks raw archives, repaired envelopes and repaired nested
WAL/page checksums and commit digests. It reaches table-history checks using
synthetic root/table archives. CI compiles and lints all targets on stable;
coverage-guided execution remains an explicit nightly step.

## Pending acceptance tests

Core process-kill, byte-cut, checkpoint and competing-writer checks now execute.
Broader publication I/O fault injection, power-loss, backup upgrades and cross-project
authorization remain open. See the [current matrix](recovery-matrix.md).

## First increment: executed checks

On 2026-10-02, on Linux with Rust stable 1.99.0:

- Formatting, Clippy with warnings denied, workspace build and all 21 tests passed.
- Three properties ran 256 cases each, including random operation sequences.
- The final AddressSanitizer format-fuzz smoke run completed 1,332,572 executions
  in 16 seconds without a crash (configured time budget: 15 seconds).
- Source size: 999 physical Rust lines, including tests and the fuzz target;
  907 lines excluding blank lines and comment-only lines. Documentation,
  manifests, lockfiles and generated build output are excluded.

These observations apply to this increment, not later revisions or the pending
transaction, recovery and security acceptance gates.

## Table increment: executed checks

On 2026-10-03, on Linux with Rust stable 1.99.0:

- Workspace and fuzz-target formatting, Clippy with warnings denied, workspace
  build and all 58 tests passed.
- Eight CPU properties run 256 cases each; the persistent CRUD model property
  runs 32 cases and reopens the file after each generated operation.
- Schema, primary-key, row/type/nullability and global capacity constraints are
  exercised. Invalid operations leave stored bytes and current state unchanged.
- Subprocess tests run the actual table CLI and protect managed files from raw
  mutations. Strict replay rejects semantically invalid history with valid CRC.
- The catalog/event AddressSanitizer smoke run completed 4,053,165 executions in
  16 seconds without a crash (configured budget: 15 seconds).
- Source size is 2992 physical Rust lines; 2743 excluding blank/comment-only lines.
  The two new code increments add 1003 and 990 physical lines respectively.

These are normal-reopen, validation and bounded fuzz checks. They do not prove
durability under process termination, power loss or torn page writes. Stage 2
remains open; this table increment has no integrated transactional recovery.

## Standalone WAL increment: executed checks

On 2026-10-03, workspace formatting, Clippy with warnings denied, build and all
76 tests passed. The new block contains 998 physical Rust lines. It exercises
every byte cut within a two-page batch, valid checksums with invalid semantics,
64-case properties, bounded allocations, owner-only permissions, lock ownership,
explicit rollback and dropped pending batches. A subprocess is forcibly killed
after a synced commit and after synced uncommitted page frames; reopen preserves
the former and excludes the latter. The process tests use a separate executable
to avoid transient file-lock inheritance by another test's fork/exec.

The WAL AddressSanitizer smoke run completed 1,554,154 executions in 16 seconds
without a crash (configured budget: 15 seconds). Total Rust source size is now
3990 physical lines. Table integration, checkpoint crashes, concurrent transaction
sequencing, backup/restore and real power-loss testing remain open.

## Managed transaction increment: executed checks

On 2026-10-03, formatting, Clippy with warnings denied, build and all 93 tests
passed. This block adds 996 physical Rust lines; the source total is 4986.
Multi-table batches, strict abort-on-write-error, rollback/drop, no-op commits,
page spill, checkpoint replacement, owner-only permissions and identity checks
are exercised. Recovery rejects rewritten history and invalid relational events
even when journal and page CRCs are valid. Read-only OS handles reproduce real
write failures and verify poisoned writers and unknown commit outcomes.

A 32-case persistent transaction property compares generated committed/rolled-back
batches against an independent map and reopens after each batch. Checkpoint damage
and leftover temporary files cannot alter committed rows. These tests do not yet
cover killing the integrated table engine during commit/checkpoint or a full
concurrency, backup/restore and power-loss matrix.

## Integrated recovery and CLI increment: executed checks

On 2026-10-03, workspace/fuzz formatting, Clippy with warnings denied, build and
all 105 main tests passed. Four subprocess helpers are ignored in the parent
runner and invoked by real kill/concurrency tests. This block adds 976 physical
Rust lines; the total is 5962. CLI tests execute managed CRUD, every batch operation,
rollback, abort, limits, value-redacted errors and explicit legacy compatibility.

Integrated recovery checks all 12480 cut positions inside a two-page transaction.
The process matrix kills staged writes, synced uncommitted frames, acknowledged
commits, continuous writers at four thresholds, and checkpointing before/after
rename. Four competing processes restore all 80 increments without lost updates.
Missing/corrupt WAL fails closed even when a stale checkpoint exists.

The managed-recovery AddressSanitizer smoke run completed 587,298 executions in
16 seconds without a crash (configured budget: 15 seconds). The target also repairs
nested checksums and commit digests to reach relational validation. The full
reliability and security gates remain open; see the recovery matrix.

## Backup library increment: executed checks

On 2026-10-03, workspace/fuzz formatting, Clippy with warnings denied, workspace
build and all 122 main tests passed. This block adds 983 physical Rust lines;
the total is 6945. Verification rejects every single-byte mutation and every
truncation of a valid archive. Tests cover CRC-preserving invalid headers,
recomputed outer hashes with invalid WAL, rejected uncommitted tails, bounded
reads, owner-only permissions and no-clobber outputs including symlinks.

Restore compares schemas and rows, all supported types, Unicode text primary
keys, nulls, binary values, schema identity history and new commits after reopen.
Source/backup bytes remain unchanged. External source corruption/truncation
prevents export and poisons the live owner. Two properties run 32 cases each.
Interrupted backup/restore publication, CLI and a backup fuzz target follow.

## Backup CLI and fault-boundary increment: executed checks

On 2026-10-03, workspace/fuzz formatting, both Clippy suites with warnings denied,
workspace build and all 140 main tests passed. Five ignored subprocess helpers
are invoked by their parent tests and are excluded from the main count. This block
adds 1000 physical Rust lines; the total is 7945, or 7343 without blank/comment-only
lines. CLI tests execute backup, verify, restore, new writes and Unicode paths.

The publication matrix kills both backup and restore after staging sync and
after publication before parent sync. Final outputs are absent or complete;
source/archive bytes survive and retries use new destinations. Competing
publishers produce exactly one verified winner. A regression reproduced before
the fix verifies that failure to open the parent after restore rename reports
unknown durability and preserves the already published database.

Deterministic faults wrap the real locked WAL file: short and interrupted writes,
zero writes, disk exhaustion inside page/commit frames, sync errors before/after
underlying sync, failed uncommitted sync, rollback truncation, tail sync and
export reads. Earlier ACKs survive; affected owners reject further writes until
reopen. A failed commit sync can leave a complete committed transaction without
an ACK, so outcome inspection remains necessary. These tests do not emulate
power loss. The new 32-case restore property compares generated CRUD and rollback
history against an independent map, including new writes after restore.

The final backup-archive AddressSanitizer smoke run completed 392,375 executions
in 16 seconds without a crash (configured budget: 15 seconds). This is a bounded
smoke run, not a completed fuzz campaign or security audit.

## Self-contained compaction increment: executed checks

On 2026-10-03, both formatting/Clippy suites, build and all 161 main tests passed;
five subprocess helpers remain excluded from that count. This block adds 1002
physical Rust lines, bringing the total to 8947. WAL version 2 stores a complete
baseline and preserves the original transaction anchor. Every byte cut/mutation
inside a two-page baseline fails closed; every cut in a later transaction keeps
the complete baseline and excludes the uncommitted batch.

Tests cover mismatched header/frame versions, baseline ordering/count/digest,
bounded inputs, more than 256 baseline pages, original table IDs/history, later
commits, stale/damaged/missing checkpoints and independent backup restore of both
WAL versions. A 64-case baseline property and the 32-case committed-map model
exercise generated anchors/pages and intermittent compaction respectively.
The actual CLI compact/backup/verify/restore sequence executed with synthetic data.

A regression reproduced the ownership gap after replacing the WAL inode and
passed after stable directory locking. A repeated full run also reproduced a
crash-harness race from brief fork inheritance of another parent test's handles;
those two parent tests now serialize process launches within their executable.
Compaction publication kill/fault tests follow in the next recorded increment.

AddressSanitizer smoke runs with synthetic version-2 seeds completed 1,275,837
WAL, 398,744 managed-recovery and 208,375 backup executions, each in 16 seconds
with a configured 15-second budget and no crash. These are bounded smoke checks.


## Compaction crash, capacity and compatibility increment: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 179 main
tests passed. Six subprocess helpers are excluded from the main count. This block
adds 965 physical Rust lines; total source size is 9912, or 9199 excluding blank
and comment-only lines. Parser code is unchanged from the preceding fuzz runs.

Eight subprocess cases kill compaction at staged, renamed, directory-synced and
returned boundaries, from both source versions. Exact pages, rows, identity and
transaction IDs recover; new commits and retry compaction work. Four parent-sync
fault cases report unknown maintenance durability and forbid further owner work
until reopen. Ownership remains exclusive before and after rename.

The actual 64 MiB bound is reached for each WAL version. The refused write changes
no bytes/state; compaction reduces repeated images and new acknowledged writes
survive reopen. Four competing processes preserve all 80 updates while performing
16 compactions. Existing staged/page/commit/streaming crash tests and all 12480
integrated cut positions now run against both WAL versions. Backup/restore kills
also cover both payload versions. Actual CLI cases cover compaction, Unicode
paths, unchanged boundaries, both archive versions, later writes and safe errors.

Baseline tests reject catalog/key/type/history violations despite valid CRCs and
commit digests. Invalid selected logs cannot adopt a valid staging file or cache.
An ignored version-2 tail disappears without entering state. An externally changed
valid snapshot is rejected by a live owner; this is a consistency check, not file
authentication. A 32-case property verifies canonical repeated baselines, all
supported types, Unicode text keys, rollback and new commits. Frozen hashes from
pre-version-2 synthetic root/table archives verify byte-compatible version-1 output
with only random database identity normalized. Staging symlinks cannot redirect
writes; failed identity/replay opens release ownership for explicit operator repair.

These checks do not simulate physical power loss or complete the stage-2/security
gate. Wider filesystem failure injection, long campaigns, history vacuuming and
stable upgrade policy remain open.


## Original index foundation: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 189 main
workspace tests passed; the six subprocess helpers remain outside the main count.
This increment adds 1064 physical Rust lines, bringing the total to 10976, or
10219 excluding blank/comment-only lines. Existing table/WAL codecs are unchanged.

Ten new index tests cover recursive leaf/internal/root splits, all lookups and
linked-leaf ranges over 1200 reverse-order keys, page-image reopen, signed-i64
boundaries, mixed UTF-8 keys, maximum-size keys and an empty root. Actual arena
capacity refusal preserves exact prior pages/root and imports successfully.
Every byte cut/single-byte mutation of a synthetic leaf fails. Repaired CRCs do
not hide malformed headers, pointers, key tags/lengths, UTF-8, duplicates or tails.
Whole-tree imports reject bad roots, missing/orphan pages, underfull children,
wrong separators, dense-ID violations and mismatched leaf chains.

A 48-case property generates up to 399 insert/duplicate/lookup operations over
integer and Unicode text keys, comparing counts, point queries and ranges against
an independent sorted-map model. It validates topology after every operation and
periodically exports/imports complete images.

The new `index_pages` AddressSanitizer target exercised raw and checksum-repaired
pages and complete bounded trees, seeded with three synthetic valid trees.
It completed 328695 executions in 16 seconds with a configured 15-second budget
and no crash. This is a smoke check, not a sustained fuzz campaign. Index mutation
has no table/WAL publisher yet; existing crash tests do not establish durable
index guarantees. At that increment, SQL, deletion/merges and integration
acceptance remained pending; maintenance follows below.

## Index maintenance and version-1 compatibility: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 204 main
tests passed; six subprocess helpers remain outside the main count. This block
adds 991 physical Rust lines and removes/replaces six, a net increase of 985.
The total is 11961 physical Rust lines, including tests and fuzz targets.
No database/WAL codec or table primary-key behavior changes in this increment.

Fifteen new tests cover bulk boundary sizes through 10000 keys, 256-byte UTF-8
keys, rejected inputs, pointer-only replacement, leaf and internal rotations in
both directions, cascading merges and root collapse. Complete deletion in three
orders over 1200 keys ends at the canonical empty tree and permits new insertion.
Actual arena exhaustion followed by deletion/reopen permits fresh splits; a full
10000-entry bulk tree permits replacement and recovers entry capacity after deletion.
Two 48-case properties check mixed CRUD/ranges/reopen and bulk versus incremental
construction against an independent sorted map.

Frozen SHA-256 digests obtained by executing the published d75751b implementation
cover root metadata and exact pages of empty, 256-key multi-level and mixed
Unicode trees. The current implementation reproduces those bytes and can import,
replace and delete their entries. This is synthetic byte compatibility, not a
database-file upgrade policy or stable durable index format declaration.

AddressSanitizer smoke runs completed 433 `index_operations` executions and
374641 `index_pages` executions, each in 16 seconds (configured budget: 15 seconds),
without a crash. Operation seeds cover full ascending/descending/interleaved
deletion, replacement/reinsertion and text keys; an independent model checks each
operation and periodic image import. The page target covers raw and repaired-CRC
inputs. The short operation run has limited throughput and is not a long campaign.

Deletion may renumber index arena IDs; external row pointers are unchanged.
Table/WAL integration, durable root publication, concurrent readers and power-loss
acceptance remain open. See [ADR 0010](adr/0010-index-maintenance.md).

## Original SQL parser: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 212 main
tests passed; six subprocess helpers remain outside the main count. This increment
adds 1044 physical Rust lines; total source size is 13005. No storage codec changes.

Eight new tests cover the accepted script/AST, boolean precedence and qualified
joins, all literals/parameter positions, malformed/unsupported syntax, offsets
without secret contents, and byte/token/statement/column/tuple/depth boundaries.
Two 64-case properties check arbitrary Unicode/truncations and escaped text with
signed-i64 values and separate parameter references. A failing regression first
reproduced an overdeep AST from flat AND followed by OR, then passed after checking
the actual combined tree depth during construction.

The `sql_parser` AddressSanitizer smoke run completed 906624 executions in 16
seconds with a configured 15-second budget, without a crash. Six synthetic seeds
cover valid DDL/CRUD, joins, transactions, quoted injection-like text, extreme
integers and depth boundaries. Parsing is not execution; planner, executor and
durable-index integration gates remain open. This is a smoke run, not a security audit.

## Managed SQL execution and CLI: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 219 main
tests passed; six subprocess helpers remain outside the main count. This increment
adds 1145 physical Rust lines and replaces one, a net increase of 1144. Total:
14149 physical Rust lines, or 13286 excluding blanks and comment-only lines.

Seven new tests execute real SQL DDL/CRUD, qualified joins, projections, multi-key
ordering, limits, separate typed bindings and transaction control. Three-valued
null truth tables and explicit null placement execute. Failed scripts and explicit
rollback preserve exact committed WAL bytes and state, including errors after a
staged insert/update. Semantic resolution runs even on empty/LIMIT-0 input.
Alias hiding, ambiguous/self-join qualifiers, missing bindings, wrong types,
nonfinite/oversize bindings and unsupported primary-key assignments fail safely.

A real 400-row join exceeds the work budget and rolls back earlier staged changes;
multirow writes exceed the existing transaction event cap without durable changes.
Plans resolve direct primary-key equality, scans and bounded nested-loop joins.
Checkpoint/compaction reopen preserves SQL-created rows and transaction numbers.
A 32-case independent committed-map property generates SQL batches, duplicate
errors and rollback, reopening after each batch. The actual CLI executes, binds,
explains, rolls back, emits no result on failure and refuses unchanged legacy files.

Parser code is unchanged from its recorded ASan smoke run. New executor modules
compile under the fuzz manifest but do not yet have an execution fuzz campaign.
Retained-output budget boundaries, dedicated SQL process-kill/backup tests and
wider semantic/fault checks follow; durable index and production gates remain open.

## SQL recovery, real budgets and read execution: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 234 main
tests passed; eight subprocess helpers are invoked by their parents and excluded
from that count. This increment adds 973 physical Rust lines and replaces/removes
22, a net increase of 951. Total: 15100 physical Rust lines, or 14213 excluding
blank/comment-only lines. Database/WAL/index codecs are unchanged.

Fifteen new tests include six SQL writer kills at ACK thresholds 5/20/60, across
both WAL versions. Each recovered script's insert and update remain atomic, all
received ACKs survive, committed IDs form a complete prefix and new writes work.
Complete commits with an unreceived response may also survive; this is not evidence
of uncommitted data. A separate post-rollback kill preserves exact WAL bytes and
the original rows. These subprocess tests do not simulate physical power loss.

SQL-created Unicode text keys, booleans, floating point, bytes, NULLs and rollback
state survive independently verified backups/restores from both WAL versions.
Restored copies accept new SQL commits without changing the source. Actual 2800
maximum-text rows exceed intermediate/shared-output byte budgets; prior staged
writes roll back. A 101x101 join reaches the real intermediate-row cap. Boundary
256-event scripts succeed, while an additional event discards the whole script.

A regression first reproduced a work-limit failure for 64 key-targeted writes on
a 6000-row table. UPDATE/DELETE now reuse primary-key equality lookup, preserving
the complete predicate check without scanning unrelated rows. Predicate evaluation
borrows validated values. Pure snapshot SELECT rejects writes/control/multiple
statements and preserves detached pages after later committed writes.

Two 48-case properties independently model generated filtering/projection/null
sorting/limits and self-join pairs. Additional tests cover every catalog type,
strict coercion refusal, parameter/row bounds, maximum 3072-byte text primary keys,
nonfirst primary-key columns, named inserts and fresh table IDs after committed
drop/recreate versus rolled-back/failed DDL.

The new `sql_execution` AddressSanitizer smoke run completed 195618 executions in
16 seconds (configured 15 seconds), without a crash. It evaluates raw seeded SQL
on fixed validated synthetic snapshots and compares generated parameterized range
queries with an independent numeric model. No filesystem writes occur in this
target. Wider fault/crash/fuzz campaigns, durable index integration, server/project
authorization and production gates remain open.

## Isolated projects and scoped API keys: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 248 main
tests passed; eight subprocess helpers remain outside the main count. This block
adds 897 physical Rust lines, total 15997. No database/WAL/index codec changes.

Fourteen new tests cover 256-bit key issuance and fixed-size digest comparison,
redacted debug/errors, 128-bit IDs, cross-project key denial, independent SQL data,
atomic rotation/reopen, private modes, traversal IDs/labels and symlink rejection.
Actual capacity reaches 128 projects and refuses the next without a new directory.
Thirty-two same-project requests complete without lost inserts. A failing regression
first reproduced root ownership release while a request remained; capabilities
now retain the root inode owner until execution/drop. A 24-case rotation/reopen
property preserves rows and refuses all former keys.

Metadata tests reject every truncation of a synthetic envelope and repaired-CRC
unknown versions, wrong IDs, bad names, hash lengths, zero epochs and extra fields.
Epoch overflow preserves bytes; metadata symlinks cannot change outside targets.
Incomplete staging is ignored/preserved; unrecognized committed entries fail.
Two 64-case properties exercise generated semantic boundaries and arbitrary bounded
bytes. The new `project_metadata` ASan smoke completed 818421 executions in 16
seconds (configured budget 15 seconds), without a crash, including repaired CRCs.

This is a synchronous registry/key library. HTTP transport, administrative network
authentication, network limits, registry publication kill/fault tests and complete
platform backups are still pending. User/password/session/role and production gates
remain open; the local private filesystem owner is trusted.

## Bounded HTTP transport: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 259 main
tests passed; eight subprocess helpers remain outside this count. This block
adds 1081 physical Rust lines and removes/replaces two, net 1079; total 17076
physical Rust lines, or 16146 excluding blanks/comment-only lines.

Eleven new tests execute separate administrator/project scopes, create/list/key
rotation, literal parameter binding, independent data and restart, exact script
rollback, traversal IDs and strict generic input errors. Actual 65537-byte bodies
fail; body completion times out after five seconds without writes. Four pending
bodies exhaust worker permits; cancellation releases them. The exact 120-attempt
IP boundary, forged forwarding headers, 4096-peer capacity and window reset run.

A failing regression first reproduced registry waiting blocking the reactor;
async mutex waiting fixes it. Real TCP requests commit data before graceful drain,
release ownership and reopen the acknowledged transaction. Binary tests reject
missing/invalid secrets before directory creation, stop cleanly on SIGTERM and
prove logs omit tokens, project IDs, query strings and bodies. Initialization's
transaction ID is 1; test assertions account for this existing behavior.

A separate live JSON Schema check validated 11 actual responses across all seven
OpenAPI route patterns, including create, SQL, explain, status, key rotation and
400/401 responses. Local references resolve. Fuzz targets compile/lint with the
updated locked transport dependencies; metadata/parser codecs are unchanged from
recorded ASan runs. No new ASan campaign is claimed for HTTP.

Four worker permits do not limit all accepted network connections. Concurrent
commit-disconnect campaigns, publication crash/fault tests, full platform backups,
security audit, broad load checks and production readiness remain open.

## Registry publication and network recovery: executed checks

On 2026-10-03, all 270 main tests, both format/Clippy suites and workspace build
passed. Nine subprocess helpers remain outside the main count. This increment
adds 976 physical Rust lines and removes/replaces six, net 970. Total 18046 physical
Rust lines, or 17103 excluding blanks/comment-only lines. Storage codecs unchanged.

Fourteen registry kills cover six creation boundaries and four rotation boundaries
for both underlying WAL versions. Incomplete staging stays unadopted; published
metadata is complete with exactly one epoch. Acknowledged keys authorize after
restart. Existing project WAL bytes/rows remain unchanged. Five injected sync
failures distinguish pre-publication refusal from post-rename uncertain outcomes;
uncertain owners refuse later operations until reopen. Already accepted capabilities
retain their documented right to finish. Lost returned keys can be replaced by an
administrator without changing the database.

Six actual binary/TCP writer kills at response thresholds 5/20/60 for WAL 1/2
preserve every received complete response and a gapless prefix of whole SQL scripts.
Anchor updates and inserts recover atomically; untouched siblings are byte-identical
and new writes work. Separate rollback/rejected-script kills preserve exact WAL.
Four TCP clients concurrently insert 32 rows across two projects, preserving each
project's complete transaction sequence and rejecting staged duplicate scripts.

SIGTERM drains an accepted partial body and its committed write before releasing
root ownership. A controlled started blocking task survives request cancellation,
keeps its permit/root owner and publishes durable rows without a response. Missing
or corrupt WAL returns generic 503 while a healthy sibling remains available;
checkpoint data cannot mask journal loss. HTTP ignores damaged optional cache;
only an explicit later checkpoint replaces it. Actual private logs contain no
project IDs, keys or SQL. These tests do not simulate physical power loss.

Filesystem tests around subprocess launch are serialized: fork briefly inherits
unrelated file descriptions until exec. Parallel harness failures first reproduced
this ownership interference; strict engine locks remain unchanged. Production
builds contain no kill/fault environment hooks. Wider random disconnect/load,
publication I/O/media failures and complete platform backup gates remain open.

## TypeScript SDK: executed checks

Strict TypeScript 7.0.2 compilation, Prettier checks and 11 unit tests pass.
Seven actual HTTP/WAL integration cases (including the parent) pass without skips:
SQL types/NULLs/literal parameters, CRUD, rollback, independent project keys,
rotation, unsafe i64 unknown commit outcome and process restart. Three failing
regressions first reproduced arbitrary peer-code reflection, uncancelled declared
oversized bodies and valid 127-byte join labels, then passed after fixes.

The SDK adds 1189 physical TypeScript/JavaScript source lines, including tests.
Rust remains 18046 physical lines; combined source is 19235. Manifests, lockfiles,
docs, installed dependencies and generated dist are excluded. The prior 270 Rust
main tests/nine helpers remain unchanged. SDK checks execute in GitHub Actions;
no npm release, browser verification or production claim is made.

## Experimental containers: executed checks

On 2026-10-03, the locked original server/CLI built in the pinned multi-stage image.
The real Docker 29.8.2 rootless daemon used cgroup v2/systemd delegation, Compose
5.6.0 and buildx 0.37.2. The Python standard-library probe verifies actual UID
10001, read-only root, private directory permissions, loopback binding and both
configured and applied 512 MiB/one CPU/64-process limits. Ruff 0.16.10 formatting
and lint checks pass. The probe adds 511 physical Python source lines; Rust stays
18046 and SDK source 1189, total 19746 excluding docs/config/dependencies/output.

Real scoped HTTP SQL, literal parameters, rollback/rejected scripts and key rotation
pass. Offline CLI backup/verify/restore, refusal to clobber existing outputs,
restored new writes, WAL-2 compaction and same-volume recreation preserve expected
source/sibling rows, keys and transaction IDs. SIGKILL during a response-counted
writer preserves every fully observed ACK and a gapless complete-row/transaction
prefix; new commits work after reopen. Deliberate sibling journal corruption fails
closed with 503 while the healthy project and liveness remain available. Private
request content is absent from logs. Only the probe's own synthetic volume is removed.

SDK integration also runs against the actual release container; its separate
native-process restart subtest is skipped in external mode because the probe
controls container restart. The prior native seven-case suite remains recorded
above. All 270 Rust main tests/nine helpers are unchanged by this packaging step.
CI now runs the real container probe. Arbitrary version upgrades, full platform
backups, remote TLS, broad load, physical power loss and production audit stay open.

## Stable index snapshots and write sets: executed checks

On 2026-10-03, both formatting/Clippy suites, workspace build and all 287 main
tests passed; nine subprocess helpers remain outside that count. The block adds
988 physical Rust lines and removes/replaces three, net 985. Rust total is 19031,
or 18046 excluding blanks/comment-only lines. SDK remains 1189 and Python probe
511; combined source is 20731. Config/docs/dependencies/generated output are excluded.

Seventeen new tests verify stable survivor IDs, sparse root collapse/empty leaf,
actual 1024-page arena exhaustion and hole reuse, largest Unicode keys, exact
canonical snapshots and old dense/frozen-image compatibility. Every cut and byte
mutation of an 8192-byte snapshot fails. Repaired header/page CRCs cannot hide
wrong sizes, counts, reserved bytes, roots, separators or missing targets.

Exact-base fingerprint/revision-bound deltas replay point replacement, splits,
merges, retirement and root changes. Stale/different bases, repeated/overlapping
changes, oversized sets and forged final topology refuse without changing the
base. Two 48-case properties exercise mixed operations against an independent map
and bounded arbitrary snapshot input. A test-case correction changed the page-size
mutation to actually modify its original zero low byte; no engine defect was masked.

The new ASan `index_snapshot` smoke completed 324086 executions in 16 seconds
(configured budget 15 seconds), without a crash. Three canonical synthetic seeds
reach empty, branch and sparse multilevel trees; the target repairs nested CRCs
and checks whole snapshot/delta replay. This increment is a codec/arena API with
in-memory atomic write sets. File publication, row ownership/key limits, managed
WAL participation, power-loss and production gates remain pending.

## Standalone index filesystem publication: executed checks

On 2026-10-03, both format/Clippy suites, workspace build and all 302 main tests
pass; ten subprocess helpers are outside that count. This block adds 1017 physical
Rust lines and nine Python probe lines. Totals: Rust 20048 (19020 without blank/
comment-only lines), SDK 1189, Python 520, combined 21757. Docs/config/lockfiles,
dependencies and generated files are excluded.

Thirteen unit cases cover private no-clobber publication, ownership across active
inode replacement, path/hard-link/symlink/permission boundaries, staging refusal,
truncation/corruption/oversized reads, stale bases and poisoned owners. Nine real
creation/replacement process kills include returned ACKs; complete selected
snapshots reopen and accept new revisions. Ten before/after injected sync errors
distinguish unchanged selection from unknown post-rename outcomes. Two actual
process writers preserve 20 increments and revision 21. A 32-case model reopens
after each generated operation; a binary CLI case checks every new command and
rejected-write preservation without reflecting input.

A failing regression first reproduced writes following a replaced root path
instead of the owned inode. Device/inode and private-path checks fixed it; moved
and replacement directories remain unchanged on refusal. Release code contains
no test pause/fault environment hooks. A full release-image rebuild and actual
Docker/Compose probe, including new index CLI and SDK, pass. A local tmpfs quota
initially stopped the image build; moving the isolated test engine cache to disk
resolved it. No passing container run was claimed for the failed build.

The existing snapshot codec is unchanged from its recorded ASan run. This is a
standalone full-snapshot publisher; table/WAL root publication, longer table
keys, row-pointer lifetime, media/power-loss and production gates remain open.

## HTTP extension-method redaction: executed regression

On 2026-10-03, a failing actual TCP regression reproduced arbitrary extension
methods entering structured logs, including a synthetic 64-character credential
used as a method. A static whitelist now emits standard method labels or OTHER.
Both authorized 405s and denied 401s are covered; route-pattern/status logging
remains useful while private method text is absent. This preserves HTTP method
handling; only log labels change.

Rust format, warning-denied workspace Clippy, build and all 302 main tests pass
(ten subprocess helpers excluded). Ruff format/lint and a rebuilt release-image
probe with SDK, restart, crash recovery and private method checks also pass.
The increment adds 48 physical Rust and 27 Python lines. Totals: 20096 Rust
(19066 excluding blanks/comment-only lines), 1189 SDK, 547 Python; combined
21832 source lines. This security repair is intentionally a small logical commit.
The broader security audit and production acceptance gates remain open.

## Offline whole-registry archives: executed baseline

On 2026-10-03, Rust format, warning-denied workspace/fuzz Clippy, locked build and
all 315 main tests pass; ten subprocess helpers remain excluded. Twelve new
registry cases and an actual CLI case cover preserved scopes/rotated epochs,
database IDs/transactions, mixed WAL versions, independent restored writes,
outstanding capabilities/direct writers, replaced roots/changed metadata, no-clobber
destinations, private/no-follow/single-link paths and oversized sparse inputs.
Every truncation and every single-byte mutation refuses. Repaired outer checksums
cannot bypass nested replay, metadata canonicality, strict ordering, duplicate
identities, reserved fields or exact lengths. An independent Python zlib/SHA
calculation supplies the frozen empty-header CRC. A 64-case property exercises
arbitrary input and repaired envelopes.

The registry_archive ASan smoke completed 1111844 runs in 16 seconds (15-second
budget), without a crash. Two private synthetic seeds cover empty and two-project
images with both WAL versions. A rebuilt release-image probe with the SDK and all
three registry CLI commands also passes. These are executed baseline checks;
dedicated publication kills/injected sync failures and physical-power-loss are
still open at this increment.

The change adds 952 physical Rust and 31 Python lines, 983 source lines overall.
Totals: 21048 Rust (19966 excluding blanks/comment-only lines), 1189 SDK, 578
Python; combined 22815. Documents, configuration, locks, private fuzz corpora,
dependencies and generated artifacts are excluded. The archive captures only the
currently implemented registry; future objects/sessions and unrelated standalone
indexes are outside its scope. No production readiness is claimed.

## Registry backup publication and restored service: executed campaign

On 2026-10-03, format, warning-denied workspace/fuzz Clippy, locked build and all
324 main tests pass. Eleven ignored subprocess helpers are invoked by parents
and excluded from that count. This block adds 909 physical Rust lines, removes
two (net 907), and adds 59 Python lines; net source growth is 966. Totals: 21955
Rust (20856 without blanks/comment-only lines), 1189 SDK, 637 Python; 23781 combined.

Twelve real process kills cover acquisition of all source owners, archive file
sync/rename/parent sync/returned ACK, and restore WAL/project/staging sync/rename/
parent sync/returned ACK. Sources and archive bytes remain exact. Unpublished
staging is never adopted; published registries reopen with scopes/epochs/IDs and
accept subsequent commits. Fourteen before/after real sync failures distinguish
unchanged output from a reported unknown complete publication. A paused capture
excludes every direct database owner together; killing it releases all owners.
Two synchronized subprocess restorers publish exactly one complete destination.

A 24-case independent three-project model generates upserts, deletes, failed
scripts, rollback, key rotation, WAL compaction and damaged optional caches.
Repeated prefix backups/restores match model rows, transactions, WAL versions and
epochs. Restored rotation/writes preserve the original registry. An actual
128-project archive restores all scopes and refuses project 129; an aggregate
sparse-file boundary refuses before opening oversized WALs.

Real source/restored server binaries verify preserved/retired/cross-project keys,
external master credentials, HTTP rows, independent rotation/writes, log redaction
and acknowledged restored writes surviving SIGKILL. The rebuilt release Docker
probe additionally serves the restored registry through its actual HTTP server
with retained scoped credentials and independent writes; all existing SDK,
container restart/crash/corruption checks pass. The parser is unchanged from its
recorded 1111844-run ASan smoke. Broader failing media, physical power loss,
encryption/streaming, stable upgrades, load/security and production gates stay open.

## Pinned registry/project/data directories: executed regression

On 2026-10-03, two failing regressions reproduced a moved registry root redirecting
creation and an accepted capability following replacement directories. Retained
no-follow directory handles and private-mode/device/inode checks fix both defects.
Nine capability combinations cover root/project/data replacement with status,
explain and writes. Actual HTTP tests verify generic failures, protected listing/
rotation/creation, unchanged old/replacement WALs, available healthy siblings and
intentional reopen. Existing creation/rotation kills, recovery/models, actual
128-project capacity, reactor responsiveness and cancellation/ownership checks pass.

Rust format, warning-denied workspace/fuzz Clippy, locked build and all 327 main
tests pass; eleven subprocess helpers are excluded. A rebuilt release-image probe
with SDK, independent restored HTTP, restart/crash/corruption also passes. Storage,
WAL, archive and metadata formats are unchanged. This increment adds 312 Rust
lines and removes 21 (net 291). Totals: 22246 Rust (21144 excluding blank/comment-only
lines), 1189 SDK, 637 Python; combined 24072 source lines. This targeted integrity
repair is a small separate logical commit. Broader hostile-admin filesystem
mutation, security/load, physical power loss and production gates remain open.

## Validated row-image locations: executed increment

On 2026-10-03, workspace/fuzz formatting and warning-denied Clippy, locked build
and all 342 main tests pass; eleven subprocess helpers are invoked by parents
and excluded. Fifteen new tests cover actual slotted positions across pages,
updates/deletes/reinsert/drop/recreated tables, divergent staged images sharing
one position, failed writes, strict JSON, extreme/forged locations, all column
types and maximum 3072-byte UTF-8 primary keys. A 48-case independent live-row
model verifies current/retired locations and reconstruction after every operation.

Managed tests bind independent byte-identical databases to different persistent
IDs, verify rollback/drop and aborted-read behavior, and preserve locations over
checkpoint loss, reopen and both WAL versions. Verified backup/restore preserves
current locations; subsequent independent copy writes retire only its own image.
Actual CLI subprocesses test resolution, compaction, stale/foreign/forged addresses,
absent rows and bounded non-echoing parse errors.

The new ASan row_locations target completed 1279930 executions in 16 seconds
(15-second budget, 20000-byte input bound, 512 MiB RSS limit), using two ignored
synthetic version-1/version-2 WAL seeds. It checks raw/repaired recovery, accepted
image reconstruction, bounded current-row samples and forged address rejection.
A rebuilt release container with SDK, restart/crash/corruption and independent
restored HTTP checks also verifies physical locations and forged-address denial.

This increment adds 1147 physical Rust lines and removes five (net 1142), plus
21 Python lines; combined net source growth is 1163. Totals: 23388 Rust (22246
without blank/comment-only lines), 1189 SDK and 658 Python; 25235 combined.
No stored formats change. Table primary-key maps still use BTreeMap; durable
table/index WAL integration, wider fuzz/security/load and production gates stay open.

## Derived primary-key B+ routing: executed increment

On 2026-10-03, workspace/fuzz formatting, warning-denied Clippy, locked build and
all 355 main tests pass; eleven child-process helpers are excluded. Thirteen new
tests exercise lazy immutable caches, historical uninitialized branches, staged
pointer/insert/delete maintenance, wrong/missing pointer denial before reads or
writes, and eight simultaneous pure snapshot readers. A failing regression first
demonstrated needless full-table rebuilding on initialized point UPDATE; staged
pointer maintenance fixes it. The existing 6000-row/64-statement work test passes.

Both 10000-integer-key and 10000-exact-256-byte-text-key tables build 768-page
trees and resolve every row without changing physical table bytes. A separate
actual incremental arena-exhaustion test admits all 10000 table rows across two
tables, rebuilds the derived cache at the real page limit and preserves lookup/
replay. Mixed 256/257/3072-byte UTF-8 keys, absent/wrong-type keys, update/delete,
drop/recreated tables, independent 48-case mutation/replay models and SQL null/
parameter/rollback/both-version backup/restore checks execute. CLI inspection
creates no index sidecar and preserves WAL and directory entries.

The new primary_lookup ASan smoke completes 434609 executions in 16 seconds
(15-second budget, 20000-byte inputs, 512 MiB RSS limit), seeded by ignored integer
and boundary/maximum UTF-8 WALs. It verifies eligible counts, reconstructed trees,
point results and unchanged physical pages after raw/repaired recovery. Rebuilt
release container/SDK/restart/crash/corruption/restore checks pass and inspect the
actual restored primary tree; seven native SDK integration/restart checks pass.

This increment adds 981 physical Rust lines and removes three (net 978), plus six
Python lines; net source growth is 984. Totals: 24366 Rust (23176 excluding blank/
comment-only lines), 1189 SDK, 664 Python; 26219 combined. Existing format bytes and
legacy behavior remain compatible. Derived routing and maintenance are in memory;
independent durable table-index pages, secondary DDL, broader load/security and
physical power-loss acceptance remain open.

## Integer primary ranges: executed increment

On 2026-10-03, format and warning-denied workspace/fuzz Clippy pass. Locked
workspace build and all 368 then-existing main tests pass; the subsequently added
nullable-range property passes with all six range tests and nine parser tests,
bringing executed main coverage to 369. Eleven subprocess helpers remain excluded.

The original B+ tree interval path verifies actual linked-leaf results, bounded
ordered keys and live physical images. Tests cover all endpoint/limit/reversed
interval cases, MIN/MAX successors, nested/reversed comparisons, aliases and
nonleading primary columns, OR/NOT/null/type/parameter rules, empty LIMIT 0,
rollback/failed scripts, reopen, both WAL versions and verified restore. A real
6000-row/64-statement narrow ranged-write script stays within its work bound.
Independent generated models run 64 interval, 48 comparison, 48 nullable predicate
and 32 ranged update/delete/error/rollback cases. CLI and current strict SDK verify
the actual primary_range explain/HTTP contract; 11 unit and seven native client
integration/restart checks pass. The rebuilt release container campaign passes.

A failing legal x-alias execution reproduced predicate parsing ambiguity with
hex byte literals. Correct lookahead and a dedicated parser regression preserve
X/x columns/qualifiers and valid/malformed bytes. After the repair, ASan parser
fuzz completes 924494 and SQL execution fuzz 282978 runs, each in 16 seconds with
15-second/512-MiB budgets. The updated primary lookup/range target completed
222550 runs with bounded raw/repaired WALs before this parser-only repair.
These are smoke checks, not full audit campaigns.

Totals: 25278 Rust (24062 without blank/comment-only lines), 1207 SDK, 664 Python;
27149 physical source lines including tests. The increment grows Rust by 912 and
the client contract/tests by 18 (combined net 930), near the requested logical
1000-line checkpoint. File formats are unchanged. Durable secondary/index WAL,
broader fault/load/security and production gates remain open.

## Bound primary-tree images: executed increment

On 2026-10-03, all 382 main workspace tests pass; eleven subprocess helpers are
excluded. Workspace/fuzz formatting, warning-denied Clippy and locked workspace
build pass. Thirteen new tests cover exact EBTI/EBIF headers, foreign database and
table binding, rollback/no-op versus sibling commits, repaired-hash missing/extra/
wrong-pointer trees, every byte cut and single-bit byte mutation, header/reserved/
revision/length corruption, and arbitrary 32-case bounded decoder input.

A real 10000-row managed table exports a 768-page tree; load resolves every key
and preserves WAL, exact relational pages and directory entries. Empty tables and
256/257/3072-byte UTF-8 boundaries retain full table compatibility. Installed
stable trees pass split/merge/root-collapse/pointer maintenance and independent
48-case mutation/export/decode/install/replay models. Both WAL versions survive
checkpoint/reopen/compaction and actual verified backup/restore with matching
images; independently mutating the restored clone retires its image only.

The new table_index_image ASan target completes 535880 executions in 16 seconds
(15-second budget, 32768-byte inputs and 512-MiB RSS limit), with ignored synthetic
EBTI and both-version integer/text WAL seeds. Raw/repaired envelopes reach nested
structural checks; recovered relational snapshots exercise export, full projection
validation and installation without page mutation. This is a smoke campaign.
The rebuilt release container and current SDK/crash/corruption/restore campaign
pass. No real user data is used.

The increment adds 1041 physical Rust lines. Totals: 26319 Rust (25067 excluding
blank/comment-only lines), 1207 SDK and 664 Python; 28190 combined source lines,
including tests. Images are explicit private library artifacts, not automatic
sidecars or independently durable table/WAL participants. Broader recovery,
load/security, format stability and production gates remain open.

## Explicit private primary-cache publication: executed increment

On 2026-10-03, all 393 main tests pass, with twelve subprocess helpers excluded.
Workspace/fuzz format and warning-denied Clippy, locked workspace build and Python
probe lint/format pass. Eleven new tests exercise explicit save/load, absence,
stale refresh, private permissions, damaged/foreign/oversized-file preservation,
symlink/hardlink/directory/FIFO denial, moved-root refusal after staging, and cleanup
through the original owner. FIFO tests return without waiting for a writer.

Eight actual process kills cover file sync, rename, directory sync and cache ACK
for creation and replacement. Every acknowledged relational row survives; active
cache files remain absent/old or complete new images, and staging is never adopted.
Eight before/after file/directory sync failures preserve WAL and permit later table
commits; post-rename uncertainty uses a distinct optional-cache error. Two actual
competing processes commit twenty rows and publish the final complete cache under
the existing database owner. A 24-case independent committed model covers cache
save/load/reopen with updates, deletes, rollback, rejected writes and no-op commits.

Compiled CLI checks exercise count-only responses, stale refresh, traversal-name
denial, damaged-output preservation/redaction and unchanged WAL. Both-version
verified backups omit disposable damaged sidecars; restore reconstructs correct
rows and can save/load fresh caches. The rebuilt release container runs the same
new save/load/backup-omission/stale/compaction checks together with its SDK, restart,
writer kill, corruption isolation and log-redaction campaign. Its first new probe
comparison mistakenly compared bytes with a string; JSON decoding repairs that
test, and the complete campaign is rerun against the same rebuilt Rust image.

The increment adds 1011 physical Rust lines and 19 Python lines (net 1030 source).
Totals: 27330 Rust (26048 excluding blank/comment-only lines), 1207 SDK and 683
Python; 29220 source lines including tests. No parser or stored image format changes;
the existing EBTI ASan target remains the decoder campaign. Automatic adoption/
refresh, orphan housekeeping, durable table-index WAL and production gates stay open.

## Bounded startup adoption and exact-history digests: executed increment

On 2026-10-03, all 408 main tests pass, with twelve subprocess helpers excluded.
Workspace/fuzz format and warning-denied Clippy, locked build and Python probe
lint/format pass. Fifteen new tests cover missing/loaded/stale startup counts,
historical versus current reports, foreign and repaired-hash wrong projections,
unsafe optional paths, rejected/skipped candidate sets and independent fallback.

A real 128-table/10000-row database saves and automatically adopts all 128 current
images within 16 MiB, preserving WAL and resolving every key. A 32-candidate set
of sparse maximum-size damaged images proves the cumulative budget and continued
loading of later small valid files. Test-only post-reservation file growth to
1 GiB and truncation reserve/read only the original length plus one probe byte;
rejection leaves acknowledged data usable. Raw trusted path semantics stay intact;
project directory authorization is not bypassed.

Exact encoded-page SHA checks cover cold/initialized historical branches, failed
events, unchanged cache installation, identical-row replacement, eight shared
readers and a 48-case independent physical-history mutation/replay model. The new
snapshot_fingerprints ASan target completes 476521 executions in 16 seconds
(15-second/20000-byte/512-MiB budgets) from ignored both-version integer/text WAL
seeds. It verifies raw/repaired recovery, exact digest, old clones, accepted event
invalidation and replay. This remains a smoke check.

Actual kills after a new table ACK and before cache refresh preserve that row for
both WAL versions; the old cache is rejected after reopen. Both-version verified
restore works without files and can adopt an explicitly copied matching clone
until independent writes retire it. Two new real-binary TCP cases preserve own
project values, scoped denial, damaged-cache availability and private log redaction;
a valid cache cannot mask a damaged mandatory WAL (503), while its sibling serves.
The rebuilt release container/SDK/crash/restore/corruption campaign passes and
checks count-only startup adoption through the compiled CLI.

Rust grows by 940 physical lines and the Python probe by seven (net 947 source).
Totals: 28270 Rust (26960 excluding blank/comment-only lines), 1207 SDK and 690
Python; 30167 source lines including tests. Existing formats and HTTP contracts
are unchanged. Automatic refresh/housekeeping, independently durable index WAL,
wider failing-media/load/security and production acceptance remain open.

## UTF-8 primary ranges

The final locked workspace run passes 422 main Rust tests; 12 ignored process
entry helpers are invoked by their parent crash tests. Workspace/fuzz formatting,
both strict Clippy checks and the locked workspace build pass. Python lint and
format checks pass. The rebuilt release container campaign, including SDK,
crash/restore/isolation/redaction checks and new real HTTP text-range reads and
updates, passes.

New tests cover all comparison directions and reversed operands, empty strings,
embedded NUL, emoji, 255/256/257/3072-byte bounds, NULL filters, aliases, joins,
LIMIT 0 validation and contradictions. Independent generated models include
48 snapshot interval cases, 48 nullable SQL comparison cases and 32 mutation/
rollback/reopen cases. A real 10000-key mixed short/long table preserves ordering
and limits; 64 narrow writes among 6000 rows remain within the execution work
bound. Forged missing/extra/wrong short-tree projections fail even if a long key
would fill LIMIT. Both WAL versions and verified restores retain the rows.
An actual compiled CLI test covers explain, reads, long-key update and rollback.

The SQL execution ASan target now independently checks generated text intervals
alongside raw SQL and integer cases. It completes 130452 executions in 16 seconds
under a 15-second, 16384-byte, 512-MiB budget without failure. This is a short
fuzz smoke campaign, not an exhaustive audit.

The initial new text-plan regression failed against the previous scan planner,
then passed after implementation. Net source growth is 962 Rust lines (+994/-32)
and 28 Python lines, or 990 total. Current totals are 29232 Rust (27897 excluding
blank/comment-only lines), 1207 SDK and 718 Python: 31157 source lines including
tests. SQL text ordering is UTF-8 byte order, without locale collation. Existing
stored formats and API contracts remain unchanged; durable index WAL, secondary
DDL, ordering pushdown and wider production acceptance remain open.

The direct text_ranges ASan target independently generates short/long UTF-8 keys,
NUL, exact 255/256/257/3072-byte bounds, unbounded/reversed intervals and row limits.
It compares the snapshot API against an independent ordered set, validates rejected
oversized/incorrect-type input, installs verified tree copies, reconstructs pages
and preserves historical clones after deletion. The first 60-second campaign
completes 52607 runs in 61 seconds with a 4096-byte input and 512-MiB RSS bound,
without failure. Fuzz formatting and strict Clippy pass. Only test infrastructure
changes after the 422-test/container-verified implementation; broader load,
power-loss and security acceptance remain open.

This separate test checkpoint adds 132 Rust lines. Source totals: 29364 Rust
(28025 excluding blank/comment-only lines), 1207 SDK and 718 Python, or 31289
physical source lines including tests. Runtime implementation and formats are
unchanged.

## Double-ended original B+ cursors

On 2026-10-04, the final locked workspace run passes 435 main tests, with 12
ignored process helpers called by parents. Workspace/fuzz format checks, strict
Clippy and the locked workspace build pass. Thirteen new tests cover exact
separator/leaf endpoints, arbitrary mixed end consumption, full 10000-key dense
and stable trees, mixed numeric/text keys, NUL/Unicode/maximum text, actual
borrowed key addresses, malformed visited pages/counts/links/roots/height and
fused error/exhaustion behavior. Independent models run 64 interval cases and
48 mutation/reimport cases. Stable holes, root collapse/reuse and active cursors
over immutable originals survive independent cloned mutations.

Actual compiled index-range CLI cases verify lower/upper/direction/default/limit
semantics, unchanged snapshot revision/images, private-path rejection, invalid
JSON/oversized bounds, no partial output and error redaction. Existing frozen
version-one images, recovery, backup, table/SQL/server tests still pass. SQL has
not yet adopted the new cursor; no HTTP contract changed.

The index_cursors ASan target independently compares generated mixed trees and
post-delete/reimport views to an ordered map, including alternating ends. Its
initial 30-second smoke completes 6134 runs in 31 seconds. A further seeded run
with four ignored synthetic 64/250/1024/2048-byte inputs completes 3654 runs in
16 seconds under a 15-second, 2048-byte-input and 512-MiB RSS bound. Both pass;
these are bounded checks, not sustained fuzz/security acceptance.

Rust source grows by 985 lines (+1015/-30). Current totals: 30349 Rust (28973
excluding blank/comment-only lines), 1207 SDK and 718 Python, or 32274 source
lines including tests. No persisted bytes or format versions change. Borrowed
live-row/SQL ordering integration and durable index WAL remain pending.

## Borrowed validated primary rows

On 2026-10-04, the final locked workspace run passes 447 main Rust tests; 13
ignored child entry helpers are invoked by parents. Workspace/fuzz format and
strict Clippy checks and the locked workspace build pass. Borrowed row identity,
mixed end consumption, integer/Text/NUL/Unicode/3072-byte bounds, nullable and
non-leading primary columns, temporary name/bound lifetimes, historical forks
and fused corruption errors execute. A 48-case independent text mutation model
checks ordered rows and replay. A real 10000-row table with 3072-byte payloads
and eight concurrent cold snapshot readers returns checked borrowed rows.

Actual transactions check staged reads, rollback, write-error abort, preserved
WAL bytes, checkpoint/cache adoption, reopen and verified restore for WAL 1/2.
Restored compaction and independent writes preserve the original view. Complete
projection verification now streams keys while retaining full topology/liveness
validation; existing allocating range contracts remain unchanged.

An oversized-key memory defect was reproduced before repair: the original
validator cloned a synthetic 128-MiB key before checking its length and aborted
under a child process address/data limit. Borrowed validation now rejects it with
ValueSize while allowing valid maximum Unicode keys and rejecting wrong types.
The isolated child allows eight MiB of growth, disables core dumps and changes no
parent limits. Its rustix process support is dev-only. Initial 32-MiB fixtures
could reuse reserved allocator space, so the reproducer uses 128 MiB; no failed
reproduction was presented as proof.

The primary_rows ASan target uses raw/repaired synthetic integer/text WAL-1/2
seeds to compare generated intervals, mixed ends, partial reverse reads,
projection verification and reconstructed snapshots. The initial campaign
completes 374937 executions; the final repaired build completes 370711, each in
16 seconds with a 15-second/20000-byte/512-MiB budget. These are smoke checks.

Net source growth is 1024 Rust lines (+1038/-14). Totals: 31373 Rust (29965
excluding blank/comment-only lines), 1207 SDK and 718 Python, or 33298 source
lines including tests. Runtime formats and HTTP contracts are unchanged. SQL
adoption, independently durable table index WAL, wider power-loss/load/security
and real-data acceptance remain pending.

## Streamed primary SQL order

On 2026-10-04 the locked workspace run passes 458 main Rust tests, with 13
ignored process-entry helpers called by parents. Workspace/fuzz formatting,
strict Clippy and the locked workspace build pass. An earlier failing regression
reproduces the 6000-wide-row ORDER BY id LIMIT 2 intermediate-byte rejection.
The repaired executor borrows primary rows, applies the full filter, retains only
projected fields and stops after enough TRUE matches. Two independent 48-case
integer/text models exercise order, ranges, OR/NOT, NULL, aliases, long/NUL/Unicode
keys and limits; typed projections preserve all catalog types and negative zero.
Unknown fields/bindings/types still fail on empty/contradictory/zero-limit reads.

Real script work and shared output budgets still reject expensive unsuccessful
filters or oversized retained results; prior writes are rolled back. Sixty-four
limited ordered reads of 6000 rows fit the work budget. Both WAL versions cover
staged reads, rollback/abort, independent old snapshots, cache/reopen and verified
backup/restore. Actual compiled CLI and TCP server cases exercise wide limited
reads, unchanged read-only WAL, scoped denial/redaction and ACK replay after kill.
Joins and non-primary ordering retain their materialized intermediate limits.

The extended sql_execution ASan target completes 34332 runs in 16 seconds, with
a 15-second, 16384-byte and 512-MiB budget. Independent nullable projections and
long-key models cover the new path. This is bounded smoke verification.

Net growth is 994 Rust lines. Totals: 32367 Rust (30925 without blank/comment-only
lines), 1207 SDK and 718 Python, or 34292 source lines including tests. No format
version or HTTP/SDK shape changed; primary_range also describes unbounded primary
ordering and sorted indicates requested ORDER BY. Durable table-index WAL, wider
recovery/load/security and production gates remain open.

## Bounded mutation selection

On 2026-10-04, the locked workspace run passes 466 main Rust tests with 14
ignored child entry helpers. One subsequently added compatibility test passes
in a focused locked run: 467 distinct main tests verified. Final workspace/fuzz
format, strict Clippy and locked build pass; the seven native SDK checks pass.

Before repair, an isolated Linux child selecting 6000 wide synthetic rows exits
with SIGABRT on a 3072-byte allocation under eight MiB of additional address/data
headroom. After repair it returns the existing transaction Limit after 257
candidate visits, verifies key-only deletion and a narrow range, then rolls back.
Staging and initial derived-cache construction precede the child memory cap;
this does not claim an eight-MiB SQL/process memory bound. Core dumps are disabled
and no parent resource limit is changed.

Public transaction checks count every staged event, preserve zero-capacity reads
and reject aborted views. Exact SQL 256/257, DDL/prior-statement accounting,
zero-match behavior and full binding preserve failed WAL/transaction numbers.
A 32-case independent mutation model covers integer/text/long/NUL/Unicode keys,
nullable AND/OR/NOT filters, rollback/errors, cache/reopen and WAL-1/2 verified
restore. Actual CLI and HTTP verify long points, generic overflow errors, scopes,
log redaction and ACK kill replay. Compatibility compares SQL and direct-library
mutations on copies with the same identity: resulting WAL/page bytes match
exactly for both key types and both WAL versions.

The new filesystem-backed sql_mutations ASan target executes 800 cases in
16 seconds under a 15-second/4096-byte/512-MiB budget. Each case constructs
258..305 synthetic rows, compares bounded scripts with an independent state,
checks arbitrary SQL error atomicity, and reopens the committed journal. Eight
synthetic seeds cover integer/text and both WAL versions. This is smoke fuzzing.

Net growth: 1033 Rust lines. Totals: 33400 Rust (31931 without blank/comment-only
lines), 1207 SDK and 718 Python, or 35325 source lines including tests. Persisted
formats and HTTP/SDK shapes are unchanged. Snapshot staging, wider fault/media/
load/security checks and durable table-index WAL remain separate work.

## Minimum Rust compatibility checkpoint

On 2026-10-04, Rust 1.89.0 executes the complete locked Linux workspace: all
467 main tests pass, with 14 subprocess entry helpers called by parents. CI adds
a separate minimum-version test job; stable format/Clippy/fuzz/SDK and actual
container jobs remain. Workflow YAML structure and quoted environment values
are validated locally. Other operating systems and older toolchains are unverified.

The first separate debug build in /tmp fails during linking because that tmpfs
runs out of space. Only the new owned build directory is cleaned. Retrying on
the main filesystem, with incremental/debug symbols disabled and two build jobs,
completes the full test suite. No source/compiler incompatibility was inferred
from the temporary-filesystem failure. Those resource settings apply to the MSRV
job, not persisted formats or the normal development profile.

A longer sql_mutations ASan check also completes 2336 runs in 61 seconds under
a 60-second, 4096-byte and 512-MiB bound with its generated corpus; no failure.
This remains bounded fuzz verification, not a completed security/power-loss audit.
The previously published code checkpoint 4f052d4 has successful GitHub stable
and container jobs. Source counts remain 33400 Rust/35325 combined; this
compatibility/documentation checkpoint adds no source lines.

## Known dependency advisory checkpoint

On 2026-10-04 cargo-audit 0.22.2 checks both lockfiles with warnings denied:
133 workspace and 105 fuzz dependency entries. Both report zero known
vulnerabilities and no warnings, with no ignored advisories or target filters.
The fetched RustSec database has 1290 advisories at revision
ef6173cbc5c50ec8166f9a5b28f07834144373ee, updated 2026-10-03. The second graph
uses that same database without another fetch. CI now adds a pinned-auditor job
for both graphs alongside stable, minimum-Rust and container jobs. Its YAML
structure and command behavior are checked locally.

This is a dated known-advisory scan, not an independent review of original code,
all transitive licenses, native/system libraries, binary contents or unknown
supply-chain issues. Those security/real-data acceptance gates remain open.
Source counts are unchanged. The previous compatibility checkpoint 247f288
has successful stable, Rust 1.89 and real-container GitHub jobs.

## Owned single-database backup publication

On 2026-10-04 both locked workspace runs, stable 1.99.0 and minimum 1.89.0,
pass all 479 main Rust tests with 14 ignored child entry helpers. Workspace/fuzz
format, strict Clippy and locked build pass. SDK format, eleven unit checks and
seven checks against the real server also pass. Two parent-substitution regressions
first fail against the old publisher, then pass with descriptor-relative owned
publication and conservative identity-bound cleanup.

The new cases execute twenty sync failures before/after real fsync, eight native
external parent/staging changes, a 32-case independent row/restore model, FIFO/
device/symlink refusal, exact old archive bytes/private modes and real relative
Unicode-path CLI checks. Existing eight publication kills and competing publishers
still pass. Uncertain publication retains the complete destination; retry cannot
replace it. Source WAL/archive bytes are unchanged. This covers the single-database
publisher, not the separate registry publisher or actual hardware power loss.

The expanded sql_mutations corpus initially exceeds the 512-MiB ASan RSS limit
(517 MiB), with about 25 MiB live heap and 222 MiB freed-block quarantine reported.
That run fails and is not counted as passing. The saved input passes twenty fixed
replays. A separate ASan-enabled campaign with 32-MiB quarantine completes 1434
runs in 46 seconds at 160-MiB final RSS, retaining the 4096-byte input and 512-MiB
RSS bounds. Reduced quarantine narrows the freed-block detection window; these
bounded observations are not a whole-process memory or completed security proof.

```sh
ASAN_OPTIONS=quarantine_size_mb=32:thread_local_quarantine_size_kb=256 \
  cargo +nightly fuzz run sql_mutations -- \
  -max_total_time=45 -max_len=4096 -rss_limit_mb=512
```

Net source growth is 857 Rust lines (+961/-104), including tests. Totals: 34257
Rust (32744 excluding blank/comment-only lines), 1207 SDK and 718 Python, or 36182
combined source lines. Archive/WAL versions and HTTP/SDK shapes are unchanged.
See [ADR 0032](adr/0032-owned-backup-publication.md); wider recovery, security,
registry publication and production gates remain open.

## Owned offline registry publication

On 2026-10-04 all 490 main Rust tests pass on stable 1.99.0 and Rust 1.89.0,
with 14 ignored child entry helpers. Workspace/fuzz format, strict Clippy and
locked build pass. Seven SDK checks against the current real server also pass.
The previous single-database checkpoint bf79267 has all four GitHub jobs green:
stable/SDK, minimum Rust, advisories and the real release container.

Two registry parent regressions first fail against the old publisher. The new
guard binds parent/staging/selection identities, writes restored projects through
the original directory handle and preserves foreign/detached objects. Eight
native pre/post-rename substitutions, ancestor replacement during WAL restore,
64 isolated descriptor-release cycles and a 32-case independent scope/epoch/row
model execute. Existing registry kills, fourteen sync failures, competing restorers
and restored actual HTTP cases remain green. New real CLI cases cover empty and
mixed-WAL registries with relative Unicode paths and parent-alias refusal.

Net growth is 980 Rust lines (+1017/-37). Totals: 35237 Rust (33700 without
blank/comment-only lines), 1207 SDK and 718 Python, or 37162 combined source lines.
EMILYREG/EMILYBAK/WAL bytes and public report shapes are unchanged. Source/archived
project values, key epochs and scopes remain exact; copy writes/rotation are
independent. These are bounded namespace/cleanup checks, not hostile-admin,
whole-process load, hardware power-loss or completed production/security proof.
See [ADR 0033](adr/0033-owned-registry-backup-publication.md).

## Owned raw page creation and managed checkpoints

On 2026-10-04 all 503 main Rust tests pass on stable 1.99.0 and Rust 1.89.0;
15 ignored child entry helpers are invoked by their parent cases. Workspace/fuzz
format, strict Clippy, locked build and seven native SDK checks pass. Both
lockfiles pass the local known-advisory check without warnings (133/105 packages,
1290 cached advisories); only storage dependency edges changed, not versions.
The previous 9fb20a2 has successful GitHub stable/SDK, minimum Rust, advisory and
real release-container jobs, run 37199464107.

Three regressions fail before their fixes: raw creation parent substitution,
checkpoint redirection after moving its owned directory and changed staging
permissions/links. Initial header/page readback now rejects truncation, corrupted
bytes and a valid foreign page. Cases check detached/substituted staging, final
aliases/nonregular files, explicit directory-handle filenames and eight sync
failures before/after underlying fsync. Post-rename uncertainty retains a complete
selection and its lock is released for inspection. A 32-case independent page
model checks exact file bytes, failed sync, retry, reopen and independent appends.

Three native process kills at synced staging, rename and returned pager retain
no target or the exact complete initialized image. An orphan stage is never
adopted. Selected images reopen and accept another page. Managed checkpoints stay
in their held directory after its name moves or is replaced, for WAL 1/2; exact
WAL and foreign old-path files remain unchanged. Actual CLI cases cover relative
Unicode paths, 0600 creation, no clobber and link refusal without printing records.

This logical block adds 1011 Rust lines and removes/replaces 78, net 933. Totals:
36170 Rust (34594 excluding blank/comment-only lines), 1207 SDK and 718 Python,
or 38095 combined source lines including tests. EMILYDB/EBPG/WAL/backup bytes are
unchanged. Raw in-place writes remain nontransactional. These bounded checks do
not close hardware power-loss, failing-media, upgrade or production/security gates.
See [ADR 0034](adr/0034-owned-page-file-publication.md).

## Owned authoritative journal replacement

On 2026-10-04 all 514 main Rust tests pass on stable 1.99.0 and Rust 1.89.0,
with 16 ignored child entry helpers invoked by parent cases. Workspace/fuzz
format and strict Clippy, locked build and all seven native SDK checks pass.
The previous 5c46052 has all four GitHub jobs successful, run 37201879044:
stable/SDK, minimum Rust, known advisories and the actual release container.
Dependency packages/versions and lockfiles are unchanged in this source block.

Three independently executed old regressions first fail for directory redirection,
substituted staging and detached selected-baseline acceptance. A fourth fails
before same-inode readback checks. Before/after rename, truncation, CRC damage and
valid foreign history cannot return false success. The original authoritative WAL
is checked before starting and publishing: substitution poisons the old owner and
preserves the detached/foreign histories. Mode/link changes are refused or reported
uncertain; post-rename uncertain selection preserves the output and blocks writes.

Twelve native external parent/regular/symlink substitutions cover both WAL versions
and pre/post-rename boundaries. A 32-case independent live-row model repeatedly
moves directories during compaction, preserves foreign old-path files, checks
exact committed pages/rows, checkpoints, reopens and accepts independent later
writes. Owned-file WAL tests verify initialization at offset zero, exact descriptor
identity, original-path preservation, nonempty/private/link/type/input admission,
competing locks and lifetime until the final descriptor clone closes. Existing
eight compaction kills, four sync failures, capacity, compatibility, backup, actual
CLI/HTTP and container probes remain included in the local workspace/SDK or prior
published CI checks; the new commit's container CI is not yet confirmed.

Source growth is +1030/-24 Rust, net 1006. Totals: 37176 Rust (35560 excluding
blank/comment-only lines), 1207 SDK and 718 Python, or 39101 combined source lines
including tests. WAL/page/table/backup bytes, transaction IDs and report fields
are unchanged. Broader hardware power-loss, failing-media, upgrade and security/
production gates remain open. See [ADR 0035](adr/0035-owned-journal-replacement.md).

## Owned managed database initialization and admission

On 2026-10-04 all 530 main Rust tests pass on stable 1.99.0 and Rust 1.89.0;
17 ignored child entry helpers are invoked by their parent cases. Workspace/fuzz
format, strict Clippy, locked build and seven native SDK checks pass. Both
lockfiles pass the cached known-advisory check without warnings (133/105 packages,
1290 advisories). Only WAL's existing rustix dependency edge changes; package
versions are unchanged. The previous c67efbf has all four GitHub jobs successful,
run 37203670641, including the actual release container. This new block's container
CI remains unconfirmed until its own run completes.

Three namespace regressions fail independently against the old constructors;
another reproduces raw WAL opening through a final symlink before no-follow
admission. Eleven initialization tests cover four failures before/after directory
and parent fsync, three native kills and eight native external directory/alias
changes. An independent 32-case row model exercises both WAL versions, exact
foreign/original histories, failed open, repair, checkpoint and later commits.
Leaf substitution, parent moves, private modes, links and descriptor release are
also checked. Six owned-WAL tests cover offset-zero reading/initialization, types,
links, IDs, oversize/nonempty admission and unchanged refused bytes. Eight real CLI
tests include relative Unicode paths and alias refusal without data/path output.

The first full run exposed an existing cache test expecting admission through a
final database-directory symlink. That expectation was updated for the documented
no-follow policy; unchanged journal/cache bytes and reopening the ordinary
directory are checked. Ordinary existing directory/file modes remain compatible.
The focused rerun and both complete toolchain runs subsequently passed.

This logical block adds 991 Rust lines and removes/replaces 56, net 935. Totals:
38111 Rust (36454 excluding blank/comment-only lines), 1207 SDK and 718 Python,
or 40036 combined source lines including tests. Initial managed directories may
remain detectably incomplete; they are never silently recreated. Post-root-commit
sync/selection uncertainty retains the journal for inspection. WAL/page/table/
backup bytes and API report shapes are unchanged. Ancestors remain operator
trusted, and broader power-loss, failing-media, upgrade and security/production
gates stay open. See [ADR 0036](adr/0036-owned-database-initialization.md).

## Owned journal differential filesystem fuzz checkpoint

On 2026-10-04 the new `owned_journal` target passes stable warning-denied Clippy,
fuzz formatting and locked compilation of all fuzz binaries on Rust 1.89.0.
All six focused owned-WAL tests pass. The unchanged runtime workspace has 530
passing main tests on both toolchains from 60faebc; that commit's four GitHub jobs
are now confirmed successful, run 37207354811, including the real container.

Two AddressSanitizer campaigns finish without failure: 163953 executions in
46 seconds from bounded generated inputs, then 235964 executions in 46 seconds
after adding actual synthetic CLI-created WAL 1/2 root images. Default ASan
quarantine is retained. Input admission is capped at 20000 bytes and RSS at
512 MiB; final RSS is 285/299 MiB. The second evolving corpus reaches 8388-byte
valid seeds, complete frames, raw/repaired headers, wrong identities and byte cuts.
These are bounded smoke campaigns, not exhaustive filesystem or security proof.

The target compares all accepted recovery metadata, baseline/committed pages and
valid/discarded byte boundaries against the direct bounded decoder. It checks
nonzero descriptor offsets, ordinary mode compatibility, hard/symbolic aliases,
moved owned files and competing locks. Opening/refusal preserves exact bytes;
failed recovery releases locks. Accepted histories retain their exact committed
prefix, append another confirmed transaction, reopen and compare every page.
Only private temporary synthetic directories are used. Seeds/artifacts remain
ignored. No runtime code, dependency versions, formats or API shape change.

This separate test checkpoint adds 173 Rust lines: totals are 38284 Rust
(36616 without blank/comment-only lines), 1207 SDK and 718 Python, or 40209
combined source lines including tests. Wider recovery/power-loss/security gates
remain open; source line counts are measurements, not development targets.

## Experimental namespace/root codec

On 2026-10-05 all 544 main Rust tests pass on stable 1.99.0 and Rust 1.89.0;
17 ignored native child entry helpers are called by their parent tests. Workspace
and fuzz format/strict Clippy, locked build and seven native SDK checks pass.
Both cached dependency-advisory checks pass without warnings (134/106 packages,
1290 advisories). Only the original local codec package and its dependency edges
are added; third-party versions are unchanged. The prior 31c50c4 has all four
GitHub jobs green, run 37208062741. This new block's own CI remains unconfirmed.

The new standalone `commit-format` package has eleven focused tests and three
256-case properties. Tests freeze address/root hashes independently constructed
with Python struct/zlib; exercise all byte cuts, every single-bit change, trailing
bytes and unknown versions/domains/key types; repair CRCs before testing reserved,
identity, page, count, predecessor and overflow admission. Database-global history
and per-table primary pages never alias. Sparse root IDs and the long-text count
path remain explicit. Exact predecessor checks include real standalone tree
fingerprints, wrong namespaces/key types, root movement and independent revision/
transaction progression. These are namespace/metadata checks, not a complete
independent table/index transaction model or durable-index implementation.

The new ASan `commit_metadata` target completes 23753661 executions in 46 seconds
without failure, with default quarantine, 4096-byte input cap and 512-MiB RSS cap
(final RSS 283 MiB). Synthetic valid EBNS/EBIR seeds and repaired envelopes reach
structural field checks. Corpora remain ignored. This is a bounded smoke campaign.
The first full workspace run was interrupted during compilation by the execution
environment; its incomplete output is not a test pass. The restarted complete
stable run and the subsequent minimum-toolchain run both pass.

The block adds 1047 physical Rust lines, including tests/fuzz: totals are 39331
Rust (37597 without blank/comment-only lines), 1207 SDK and 718 Python, or 41256
combined source lines. The codec is absent from normal server dependencies; no
runtime WAL version, old page/index/backup bytes, HTTP or SDK shape change.
Root metadata alone does not prove topology, live pointers or durability. Combined
budgets, the staged state model, one commit fence, migration, power-loss and
security/production gates remain open. See [ADR 0037](adr/0037-experimental-commit-namespaces.md).

## Complete staged table/index model

On 2026-10-05 all 557 main Rust tests pass on stable 1.99.0 and Rust 1.89.0,
with 17 ignored native child helpers invoked by their parent tests. Workspace/
fuzz formatting and warning-denied Clippy, locked build and seven native SDK
checks pass. Cached known-advisory checks pass without warnings (135/106 packages,
1290 advisories); only the original local model package is added. The previous
5f10236 has all four GitHub jobs green, run 37266455924, including the actual
release container. The new model block's own container CI is not yet confirmed.

The model has eleven focused integration cases, a counter-exhaustion unit case
and a 32-case generated two-table reference-map property. It exercises insert/
replace/delete, rollback, abort after earlier successful events and invalid late
pointers. Before publication, every selected stable index is checked against
current keys/physical row images and exact root/schema/count/base metadata.
Old views, untouched table roots and refused plans retain exact state/images.
Stale prepared writers and equal-transaction divergent forks fail closed.
Other cases cover changed/missing/extra roots, foreign trees, obsolete pointers,
owner/key type/exact-base mismatch, duplicate/unstable candidates, long-text
coverage, drop/recreation namespaces, split/merge/arena reuse and 256-event bounds.

The first focused run exposed an owner-error fixture whose transaction was too
old even for codec construction. It was changed to a future owner, so preparation
now exercises the intended owner refusal. The corrected focused and both complete
workspace runs pass. No runtime engine defect was hidden or claimed fixed.

This logical block adds 1002 Rust lines, including its tests: totals are 40333
Rust (38542 excluding blank/comment-only lines), 1207 SDK and 718 Python, or
42258 combined source lines. Both prototypes remain outside normal server
dependencies. The model performs memory publication only: no file/WAL write,
durable ACK, migration or existing format/API change occurs. Its reuse of existing
physical/topology validators is explicit; the row-map reference is independent
test data. Combined budgets, full-capacity staged memory, typed retirement WAL
records, one synced fence, recovery/backup/migration and broader security/power-
loss/production gates remain open. See [ADR 0038](adr/0038-staged-table-index-model.md).

## Generated table/index model sequence fuzz checkpoint

On 2026-10-05 the new `commit_model` target passes warning-denied fuzz Clippy,
formatting and locked compilation of all fuzz binaries on minimum Rust 1.89.0.
The cached fuzz-lock advisory check passes without warnings (107 packages,
1290 advisories). Only the existing local model dependency edge is added; no
runtime source, workspace lock or third-party version changes.

Two default-quarantine ASan campaigns complete without failure: 41559 executions
in 46 seconds, then 15903 in 46 seconds after adding synthetic 32-command seeds.
The second corpus reaches the exact command bound, including all-long text keys
and rollback mixtures. Input is capped at 256 bytes/32 operations and RSS at
512 MiB; final instrumented-process RSS is 448/486 MiB. Coverage/quarantine are
part of that process, so these figures are not a runtime memory-admission proof.
The campaigns are bounded smoke checks, not complete security/load acceptance.

Each sequence compares accepted rows and eligible/excluded counts with an
independent sorted map. It exercises inserts/replaces/deletes, rollback, absent/
duplicate keys, i64 boundaries, NUL/long UTF-8 keys and exact-state retention.
Valid headers with altered predecessor fingerprints, foreign database owners
or future transaction owners are refused after earlier staging succeeds. Old
views remain unchanged. Only memory models and ignored synthetic corpora are used.

The separate test checkpoint adds 179 Rust lines: totals are 40512 Rust
(38718 without blank/comment-only lines), 1207 SDK and 718 Python, or 42437
combined source lines. The unchanged implementation's 557 main tests pass on
both toolchains from the preceding block; its own CI status is tracked separately.
No WAL write, durable index, migration or format/API change is enabled.

## Full-capacity staged model checkpoint

On 2026-10-05 the three new capacity cases pass on stable 1.99.0 (28.14 seconds)
and minimum Rust 1.89.0 (30.12 seconds); model warning-denied Clippy and workspace
format checks pass. The preceding f7c6be4 has all four GitHub jobs green, run
37268406775, including complete stable/minimum suites and the release container.
This checkpoint has 557 previously full-suite-checked tests plus three newly
checked focused cases; its own full-suite/container CI is not yet confirmed.

Real staged states reach 10000 global rows, in batches of at most 256 events.
Integer and exactly 256-byte text keys accept an index-only dense 768-page rebuild;
its EBIF image is exactly 3149824 bytes. Every short text key/value/current physical
pointer is checked. An untouched second table keeps its root and old views remain
unchanged. Exactly 3072-byte keys retain all 10000 rows/physical locations through
the relational path, while the one-page tree reports zero covered and 10000
excluded keys. Overflow inserts abort without changing the selected state.

The first expanded test compilation compared the tree's Result return value with
an Option. Assertions now unwrap successful short-key lookup and check the expected
oversized-key error on direct long-key lookup. Both complete three-case runs then
pass. Runtime source and existing formats are unchanged; this was a test fixture
compilation correction, not an engine repair or durability proof.

The checkpoint adds 190 Rust test lines: totals are 40702 Rust (38903 excluding
blank/comment-only lines), 1207 SDK and 718 Python, or 42627 combined source lines.
[Capacity arithmetic](durable-index-capacity.md) records current objects and
allocation sites without selecting a new WAL layout or claiming a measured heap
limit. Combined budgets, mixed fragmentation, shared durable publication,
recovery/backup/migration and broader production acceptance remain open.

## Combined index images and canonical hash allocation

On 2026-10-05 the complete stable 1.99.0 workspace run passes 574 main tests;
the subsequently added four-reader case passes separately on stable. The complete
minimum Rust 1.89.0 run includes it and passes all 575 main tests. Both have 17
ignored native child helpers invoked by parent tests. Workspace/model/fuzz strict
Clippy, all format checks, locked build, eleven SDK unit and seven native SDK
cases pass. Cached advisory checks pass without warnings (135/107 packages,
1290 advisories). Third-party versions/lockfiles are unchanged. The prior 7fec4d3
has all four GitHub jobs green, run 37269289274; this block's own CI is unconfirmed.

Two deliberately failing regressions first reproduce missing aggregate admission:
counts above 2048 were accepted, and a late excess image reached invalid-revision
validation instead of budget refusal. Combined candidate/complete-state checks now
abort the stage before that image validation. Valid states retain full row capacity:
a real 10000-row/128-table rebuild selects 1536 fragmented pages (6815744 EBIF bytes)
and resolves every current pointer/value. An independently assembled 1024-page
arena permits the exact 2048-candidate-page boundary; the next page refuses after
an earlier staged row, preserving the original state.

Independent forest properties cover 32 cases with integer/exact-256-byte UTF-8
keys and both branch grouping strategies, checking the occupancy-derived bound
of at most 1760 valid selected pages. A 256-case u128 arithmetic reference checks
accepted/refused counts and exact bytes. Actual serialized components, drop/
recreation, rollback/abort and 32 generated transaction sequences check reports
and canonical cached-state hashes. Four real reader threads retain old rows,
pointers, roots and component counts through 32 serial memory publications.

Four new fingerprint cases pass before and after refactoring. They freeze two
EBIF SHA-256 digests constructed independently with Python struct/zlib, compare
streaming fingerprints directly with full encoded bytes for sparse/full trees,
check equivalent invalid admission and exercise 32 generated mutation cases.
The new hash path does not allocate a final EBIF Vec; image vectors/topology
reconstruction still allocate. Immutable selections reuse validated canonical
hashes, while a detached mutable snapshot cannot change their cached state.

ASan `component_counts` completes 34724445 runs in 46 seconds, 64-byte input and
512-MiB RSS caps, default quarantine, final RSS 275 MiB. The updated `index_snapshot`
target completes 117599 runs in 52 seconds, 69632-byte input and 512-MiB RSS caps,
default quarantine, final RSS 426 MiB; accepted/repaired envelopes exercise exact
canonical hashes and complete delta replay. Both finish without failure. These
are bounded smoke checks. Minimum-toolchain fuzz compilation passes with locked
`cargo check`; a requested minimum Clippy invocation found that component absent,
so no minimum Clippy pass is claimed. Stable fuzz Clippy passes without warnings.

This logical block adds 1008 Rust lines and removes 14 (net 994): totals are 41696
physical Rust (39828 excluding blank/comment-only lines), 1207 SDK and 718 Python,
or 43621 combined source lines. EBIX/EBIF, hashes/delta bases, runtime WAL selection,
backup/API/SDK shapes and third-party versions remain unchanged. Aggregate image
admission/reporting does not enforce a heap quota or reserve memory for four server
workers. Decoding, retained views, replay/compaction, complete WAL-byte budgeting,
one durable fence and broader production gates remain open. See
[ADR 0039](adr/0039-combined-model-index-images.md).


## Opt-in allocation diagnostics

On 2026-10-05 the new release diagnostic passes five actual child-process cases
and ten report admission/property cases. The ten decoder cases also pass on
minimum Rust 1.89.0; its feature-enabled all-target check passes. Default workspace
and opt-in strict Clippy, locked workspace build, stable fuzz Clippy and both
format checks pass. Minimum fuzz-bin compilation passes. The prior 575 engine
cases remain checked from the preceding implementation block; this change adds
only a separate opt-in diagnostic and does not change runtime engine behavior.
Default tests do not activate its five feature-gated native cases; the added
fifth CI job explicitly enables them. No default zero-test binary run is counted.

The new report checks reproduce then repair acceptance of an impossible block
count at peak. They preserve valid decreases in blocks-at-byte-peak, refuse
regressed allocation/byte-peak counters, test checked differences through u64::MAX,
verify exact modes/components and reject unknown fields at every nested object.
Three 128-case properties cover bounded shapes, row-dependent image limits and
arbitrary JSON input. Four preserved actual 10000-row reports pass the bounded
Rust decoder. Separate actual binary cases check old/new values, cleanup, digest
equality, lower streamed traffic and error redaction. Reports are unsigned
observations, not authenticated measurements or numeric admission proofs.

The seeded ASan `profile_report` campaign completes 6540395 executions in 46
seconds with 8192-byte input and 512-MiB RSS caps, default quarantine, without
failure. Fuzz campaigns are bounded smoke tests. Workspace/fuzz cached advisory
checks pass without warnings (152/123 packages, 1290 advisories). Existing
third-party versions are unchanged; pinned dhat 0.3.3 and its profiler dependencies
are added only for the diagnostic. Normal server/CLI dependency graphs exclude
the diagnostic and its allocator. No authored unsafe code is introduced.

Four actual local release workloads verify 10000-row shapes. The four held
long-key models peak at 985295630 requested bytes and drop from 985283384 to
572932520 current bytes after old-view release. Construction/publication are
serial, so this does not measure overlapping worker transients. A 768-page
short-key fingerprint comparison requests 21805352 full-encoding bytes versus
18651432 streaming bytes; hashes match. Separate process RSS and reproduction
are recorded in [allocation profiles](model-allocation-profiles.md). Timings,
encoded components, requested heap and process RSS remain distinct quantities.

This block adds 1040 physical Rust lines: totals are 42736 Rust (40809 excluding
blank/comment-only lines), 1207 SDK and 718 Python, or 44661 combined source lines.
WAL/database/index formats, runtime durable selection and API/SDK shapes stay
unchanged. Heap reservation, retained-view lifetimes, replay/history/compaction,
shared durable records and production gates remain open.

The preceding 971dac8 CI attempt could not acquire hosted runners; all jobs were
cancelled before any steps. This is an infrastructure failure, not a test pass
or a demonstrated source failure. Run 37363639971 was requested again; its final
result and this block's own CI are tracked separately. Prior 7fec4d3 has all four
jobs green, run 37269289274. See
[ADR 0040](adr/0040-opt-in-model-allocation-diagnostics.md).


## Shared immutable relational tables

On 2026-10-05 all 602 main workspace tests pass in complete stable 1.99.0 and
minimum Rust 1.89.0 runs, including 17 ignored child helpers invoked by parents.
Workspace/feature/fuzz strict Clippy, workspace/fuzz formatting, locked build,
minimum feature/fuzz checks and cached advisories pass. Eleven SDK unit and seven
native SDK cases pass against the compiled original HTTP/WAL engine. The opt-in
release diagnostic passes six actual child-process tests and ten report cases.
The normal runtime does not link the diagnostic allocator. Dependencies and all
existing durable formats, hashes, ACKs and API/SDK shapes are unchanged.

A pointer-identity regression first fails on the preceding eager-copy Snapshot.
Per-table copy-on-write now shares immutable tables and physical-location maps;
first valid row mutation detaches only that table/map. Rejections preserve shared
rows, locations, exact pages and history digests. Drop/recreation keeps historical
IDs/maps separate. Four real writer threads mutate independent cloned snapshots;
they do not acquire simultaneous managed WAL ownership. Ten new database cases
include all supported value types, long keys, old borrowed rows/locations and full
10000 global rows with eight historical views and an unrelated one-row update.
A 32-case independent snapshot model checks accepted/refused/rolled-back mutations,
current/historical rows and exact physical replay through shared generations.

Four new managed cases cover begin/no-op/rollback/abort, exact WAL preservation,
old readers, commit/reopen/checkpoint/compaction under both WAL versions and a
24-case independent committed row model. The new property initially assumed key
zero stayed present after deletion; its minimized delete/reinsert-rollback fixture
exposed a test-helper unwrap. The helper now handles absent rows, and a separate
deterministic empty-table test preserves this case. This was a test fixture repair,
not an engine fault. Generated regression artifacts are ignored. Three prototype
cases check shared index-only publication, changed/unchanged table identity through
prepare/publish and exact-base refusal of a stale index-only candidate.

ASan `profile_report` completes 6478927 runs in 46 seconds, 8192-byte input/RSS512
caps, default quarantine, final RSS373 MiB. `commit_model` completes 15049 runs in
46 seconds, 256-byte input/RSS512 caps, default quarantine, final RSS496 MiB.
Both finish without failure; these are bounded smoke campaigns, not full audits.
The seeded index-only report also round-trips with the earlier preserved reports.

Actual four-model release measurements lower the requested-byte sample at begin
from 984941760 to 573235312. Index-only staging retains shared rows; dropping old
views releases 647520 bytes in that shape. Its peak675834188 still includes model
construction. Row-write mode still copies each large affected table/map and peaks
at985288974. These observations do not select a numeric quota or measure parallel
worker/replay/backup transients. See [profile follow-up](model-allocation-profiles.md#shared-table-follow-up).

The logical block adds 1030 Rust lines and removes 30 (net1000): totals are 43736
physical Rust (41777 without blank/comment-only lines), 1207 SDK and 718 Python,
or45661 combined source lines. Heap/lifetime/worker reservations, complete shared
WAL records/replay and production gates remain open. See
[ADR0041](adr/0041-shared-relational-snapshot-tables.md).

The preceding d4f476c has its minimum-Rust, allocation-diagnostic and advisory CI
jobs green, run37367805472. Stable and container jobs could not acquire hosted
runners before any steps; only those jobs were requested again. No full green
result is claimed for that run or this block until all jobs actually finish.


## Shared immutable row bodies and keys

On 2026-10-05 all 611 main tests pass in complete stable 1.99.0 and minimum
Rust 1.89.0 runs, with the 17 native child helpers invoked by parents. Strict
workspace/feature/fuzz Clippy, workspace/fuzz formatting, locked build, minimum
fuzz compilation, cached advisories and SDK eleven unit/seven native checks pass.
The release diagnostic passes six actual child-process and ten report cases.
No dependency, stored format, checksum, fingerprint, WAL fence or API shape changes.

A row-pointer regression first fails on table-only sharing. Detached maps now
retain immutable key/body Arc handles and own only structure and changed bodies.
Nine new cases include unchanged same-table row/key identity, delete/reinsertion,
64 held generations, weak-reference release, isolated owned scan mutation and
raw-file CRUD/reopen. Full 10000-row cases cover integer/256-byte/3072-byte keys,
current locations, bounded borrowed intervals and reverse traversal. A separate
32-case reference model checks successful/refused/discarded changes and exact
replay while preserving unrelated row bodies. The prior full-table-copy allocation
assertion fails because its expected cost is removed; the updated small native
workload instead caps retained map/body overhead, with independent row identity
and measured reports supporting the reduction. No functional engine fault is claimed.

The ASan model-sequence campaign completes 3947 executions in 46 seconds, input
cap256/RSS512 MiB, default quarantine, final RSS494 MiB, without failure. This
is a bounded smoke campaign. The real four-model long-key release run peaks at
581147606 requested bytes versus the prior985288974; built bytes grow to574193520.
Instrumented Linux maxRSS623532 KiB/elapsed3.88s remain separate process observations,
not admission/throughput proof. Preserved actual reports pass bounded decoding.

This small completed optimization checkpoint adds470 Rust lines and removes22
(net448), without padding toward a line quota. Totals:44184 physical Rust,42212
without blank/comment-only lines,1207 SDK and718 Python;46109 combined source lines.
Map-structure copying, derived tree copies, retained generations, replay/worker
reservation and durable-index writer gates remain open. Own CI is tracked separately;
no full green result is claimed while hosted jobs are unconfirmed. See
[ADR 0042](adr/0042-shared-row-bodies-and-live-keys.md).

## Experimental model lifetime reservations

On 2026-10-06 all 631 main workspace tests pass in complete stable 1.99.0 and
minimum Rust 1.89.0 runs, with 17 ignored native helpers invoked by their parents.
An initial stable run ended with SIGTERM before completion; it is not counted as
successful. The separate 12-case network recovery run and the subsequent complete
stable run pass. Workspace/fuzz formatting, strict workspace/feature/fuzz Clippy,
locked build and cached dependency-advisory checks pass (152/123 dependencies,
1290 cached advisories). No runtime dependency or existing stored format changes.

Twenty new cases exercise the optional ModelPool: validated count configuration,
atomic create/writer/generation reservations, duplicate identity, disabled access,
fallible reader cloning, same-generation sharing, retained historical values and
last-reader release. Prepared states keep both reservations; discard, empty/failed
prepare, aborted writes/rebuilds, unwind and foreign publication return their slots.
A dropped project cannot free its namespace while a descendant remains alive.
Equal ID/fingerprint in another pool cannot authorize publication. Automatic
original-tree rebuilds cover a non-first text primary key, 3072-byte exclusions,
drop/same-name recreation and exact successor roots.

Eight held project threads admit exactly four global writers before release/retry.
Eight callers to one project admit exactly one writer and four reader objects.
The independent 32-case operation model predicts unique current/historical
transaction generations plus pending reservations, checking refusal/counters and
all historical values after each operation. Internal poison refuses public access
while leases still clean up. Weak references confirm the actual generation/state
objects disappear after their last reader before an admission slot is reused.

The completed logical block adds 1287 Rust lines and removes one (net1286), with
45470 physical Rust lines (43411 without blank/comment-only lines), 1207 SDK and
718 Python lines: 47395 combined. This is a count-based memory prototype, not a
numeric heap/RSS, HTTP-worker, replay or WAL-buffer quota. Caller-owned copies and
raw models remain outside its boundary. No durable index or production gate is
completed. See [ADR 0043](adr/0043-bounded-model-lifetimes.md) and
[model lifetimes](model-lifetimes.md).

## Model admission sequence fuzz checkpoint

On 2026-10-06 the new `model_admission` ASan target completes 180786 executions
in 46 seconds under a 256-byte input/512-MiB RSS cap, default quarantine, final
RSS427 MiB, without failure. Its independent reference tracks project identities,
distinct generations, readers and pending writers through create/close/recreate,
clone/refusal, stage/prepare/publish, abort/discard and foreign publication. Orphan
readers and prepared operations must keep their old registration charged. Every
step checks exact counters and retained row values; complete cleanup returns zero.
Disabled capacities and invalid database identity are included. This is a bounded
smoke campaign, not a security or leak/heap audit.

All fuzz bins pass strict Clippy, formatting and minimum Rust 1.89.0 compilation.
The production Rust source/runtime is unchanged from the preceding full 631-test
stable/minimum checkpoint. This separate QA block adds 380 Rust lines; totals are
45850 physical Rust (43781 without blank/comment-only lines), 1207 SDK and718
Python, or47775 combined. No dependency/version changes; corpora are ignored.

## Validated physical image-plan checkpoint

On 2026-10-06 all 649 main workspace tests pass in complete stable 1.99.0 and
minimum Rust 1.89.0 runs, with 17 ignored child helpers invoked by their parents.
The opt-in release diagnostics pass all six real-process cases and ten report
cases. Workspace/fuzz formatting, strict workspace/feature/fuzz Clippy, minimum
fuzz/feature compilation, locked build and cached dependency-advisory checks pass
(152/123 dependencies and 1290 cached advisories).

Eighteen new cases cover independent physical replay, equal page numbers across
history/two tables, retained views, omitted unchanged roots, index-only plans,
retirement/recreation, long-key exclusions and independent row-operation models.
Every byte of real history/index images is damaged separately. Repaired checksums
cannot authorize committed history rewrites or another table's row pointer.
Exact base/next fingerprints, adjacent transactions, image/retirement ordering and
complete root coverage refuse partial or foreign state. A real dense 10000-row
arena readdresses/replays all 768 index pages; a separate case replays exactly
256 newly appended long-value history pages. The capacity fixture's initial
792-page incremental shape was corrected to an explicit dense rebuild; that
failed fixture assertion is not counted as a successful test.

The extended component-count ASan campaign completes 24453158 executions in
46 seconds with a 64-byte input/512-MiB RSS cap, default quarantine, RSS278 MiB.
The extended commit-model campaign initially fails its 512-MiB process cap at
RSS519 MiB with default quarantine: the tool reports about 240 MB quarantined
and 25 MB live allocations. The retained artifact passes one separate execution
under the same cap; that fixed-input run is not fuzzing. A subsequent campaign
with ASan still enabled, an explicit 64-MiB quarantine and 256-KiB thread-local
quarantine completes 7009 executions in 46 seconds, with the original 256-byte
input/512-MiB RSS cap and final RSS222 MiB. These are bounded smoke campaigns,
not a proof of a process-wide heap quota, a leak audit or durability acceptance.

The logical block adds 1194 Rust lines and removes one (net1193). Totals are
47043 physical Rust lines (44891 excluding blank/comment-only lines), 1207 SDK
and718 Python, or48968 combined. Lockfiles add only the model's dependency on
the original workspace storage crate; external versions are unchanged. Plans
write no files or WAL and do not escape ModelPool admission. Owned plan/replay
buffers and full-history reconstruction still need byte/transient reservations.
No new durable writer or production gate is completed. See
[ADR 0044](adr/0044-validated-physical-image-plans.md) and
[image plans](model-image-plans.md).

## Physical replay allocation checkpoint

On 2026-10-06 the opt-in release diagnostics pass nine actual-process tests and
sixteen report tests (25 total), including six integer/short/long row/index-replay
commands, retention/release comparisons and malformed real report counters.
Minimum Rust 1.89.0 passes all sixteen report tests and compiles the feature.
Strict workspace/feature/fuzz Clippy, formatting and locked workspace build pass.
The production engine is unchanged from the complete 649-test stable/minimum
checkpoint; that earlier full run is not relabeled as a new full-suite result.

The 128-case codec property now includes both replay modes. Version-two reports
check exact phases and workload-specific image counts, reject repaired impossible
totals/mode/version, unknown nested fields and all truncations, and accept extreme
consistent u64 counters without overflow. Existing version-one JSON shape and
all preserved historical observations remain accepted. Three new real release
reports pass the bounded decoder. No lockfile, external dependency or stored
database/WAL format changes.

The extended report decoder ASan campaign, seeded with the three actual synthetic
reports, completes 5474976 executions in 46 seconds under 8192-byte input and
512-MiB RSS caps, default quarantine, final RSS326 MiB, without failure. This is a
bounded smoke campaign. Reports are unsigned observations, not memory admission.

Four held long-key 10000-row states need only 16384 changed page-body bytes, but
holding independent history replay adds 573246472 requested live bytes. Releasing
the outputs restores the exact plans-built sample. Same-shape index-only replay
adds 328544 requested bytes and releases them. The instrumented global peaks/RSS
include construction; simultaneous worker transient bounds remain unmeasured.
See [preserved profiles](image-replay-profiles.md) and
[ADR 0045](adr/0045-image-plan-and-replay-allocation-diagnostics.md).

This separate diagnostics block adds 544 Rust lines and removes nine (net535),
bringing totals to 47578 physical Rust lines (45408 excluding blank/comment-only
lines), 1207 SDK and718 Python, or49503 source lines. The smaller logical commit
records a measured prerequisite rather than padding to a line target. Numeric
byte/transient reservations and the shared durable writer remain open.

## Shared canonical history append checkpoint

On 2026-10-06 complete stable 1.99.0 and minimum Rust 1.89.0 workspace runs pass
all 668 main tests, with 17 child helpers ignored by the runner and invoked by
their parents. The opt-in release suite passes all nine real-process and sixteen
report tests. Workspace/fuzz formatting, strict workspace/feature/fuzz Clippy,
minimum fuzz/feature compilation and locked build pass. No lockfile/external
dependency or stored page/WAL decoder changes.

The row/page sharing regression first fails against full reconstruction, then
passes after bounded append replay. Twelve new database cases include independent
48-case append/full-replay/map properties, preserved old tables/rows/pages,
later-event rollback, old-prefix rewriting, deleted/empty slots, gaps/order/IDs,
noncanonical packing, root/malformed events, non-first text keys and recreation.
Real 256-record/page, 10000-row, 128-table and 100000-event limits are reached;
refusal preserves the base and successful removal reclaims row/table capacity.
Existing every-byte image damage, exact namespace/base/root/state checks and
full 768-page and 256-history-page replay tests still pass. Public plan refusal
categories for committed rewrites and earlier/gapped pages remain unchanged.

The new history_append ASan campaign completes 14210 executions in59 seconds;
the updated commit_model completes 3046 in50 seconds. Both keep ASan enabled
with explicit 64-MiB/256-KiB quarantine, a512-MiB RSS cap, respective 8192/256-byte
input caps and final RSS232/201 MiB. The append target compares accepted operations
with a separate row map and complete original history replay, includes non-first
integer/256-/3072-byte text keys, discard and CRC-repaired images, and checks that
all refusals/attempts preserve the base. Soft 45-second campaign requests can
finish later while executing an input. These short campaigns are not audits.

Two preserved repeat allocation observations verify the same synthetic long/short
shapes and exact transient release. Four long-key 10000-row models now add
6941800 requested live bytes at replay instead of573246472; the image-body sum
remains16384. Requested global peak is588108552, instrumented maximum RSS623460
KiB. Numeric process-wide/transient/worker reservations are still unimplemented.
See [shared replay observations](shared-history-replay.md) and
[ADR0046](adr/0046-shared-tail-history-replay.md).

The logical block adds781 Rust lines and removes27 (net754), giving48332 physical
Rust lines (46132 excluding blanks/comment-only lines),1207 SDK and718 Python,
or50257 source lines. It commits a complete verified optimization without line
padding. No combined durable writer, real-project migration or production gate
is enabled.

## Scoped parallel replay diagnostic checkpoint

On2026-10-06 the feature-gated release suite passes32 tests: five coordination,
ten native-process and seventeen report cases. Minimum Rust1.89.0 passes all
seventeen report tests and compiles the feature. Strict workspace/feature/fuzz
Clippy, formatting and locked build pass. The production engine is unchanged
from the preceding complete668-test stable/minimum checkpoint; this separate
diagnostic run is not relabeled as a new full-core run. No lockfile or external
dependency changes.

Four real waiters exercise cancellation and poison cleanup. Actual scoped model
outputs keep exact identity/order; one foreign input refuses the group after all
workers join. Empty/mismatched/excessive groups refuse before spawn. Native tests
exercise all integer/short/long row/index modes with four workers, retained values
and bounded release, and reject parallel flags on other modes. Version-three
reports require the flag/mode; version-one/two defaults retain their JSON shapes.
The128-case codec property includes serial/parallel choices. Preserved actual
reports pass the same bounded decoder. OS thread-resource exhaustion was not
forced; cancellation mechanics are tested separately.

The extended report ASan campaign, seeded with both actual version-three reports,
completes5813997 executions in46 seconds with8192-byte input/512-MiB RSS caps,
default quarantine, final RSS334 MiB, without failure. This is a smoke campaign.
The attempted exact-baseline release assertion fails on an observed48-byte
residual; it is not counted as green. The final checks retain a bounded tolerance.
Its allocation owner is not traced; no zero-leak or numeric heap claim follows.
See [parallel observations](parallel-replay-profiles.md) and
[ADR0047](adr/0047-scoped-parallel-replay-observations.md).

This separate logical block adds371 Rust lines and removes8(net363). Totals are
48695 physical Rust(46477 excluding blanks/comment-only lines),1207 SDK and718
Python, or50620 source lines. The normal server does not enable the feature or
allocator. ModelPool byte/transient admission and the shared durable writer remain
open; real project data is not loaded.

## Parallel residue owner checkpoint

On2026-10-06 a separate local release trace on Rust1.99.0 invokes the actual
feature-gated parallel helper three times over four synthetic empty-table plans,
drops every output and then every plan/base. It observes48 additional live bytes,
with exactly one48-byte block remaining. The actual allocation stack identifies
the standard channel readiness recv_timeout's thread-local
`std::sync::mpmc::context::Inner`; no model-state allocation remains in this
minimal trace. Three groups do not grow the residue. This is targeted owner
evidence, not an exhaustive leak audit or a byte reservation.

Checked public evidence includes counts/type only. Raw local trace paths/process
identity remain outside Git. No Rust source, dependency, stored format or report
fixture changes; the preceding32 diagnostic tests and complete668-test core
checkpoint retain their original scope. Source totals stay48695 Rust/50620
combined. See [owner follow-up](parallel-replay-profiles.md).


## Complete standalone physical image envelopes (2026-10-06)

Stable1.99.0 and minimum Rust1.89.0 complete workspace runs pass684 tests each,
with17 intentionally ignored helpers and no failures. The15 new codec cases
include two independent32-case properties, every-byte cuts/damage, repaired public
digests, nested count disagreement, root/history ordering, namespace substitutions,
retirement overlap, exact fingerprints and frozen424-byte compatibility. Existing
full-capacity768-index-image and256-history-page cases now pass through actual
EBIP encode/decode before independent replay. 128 created/retired roots and
nonfirst text keys with UTF-8/NUL/256/3072-byte boundaries execute.

Strict workspace/fuzz/optional-feature Clippy, both format checks, locked workspace
build, minimum-toolchain fuzz bins and optional-feature compatibility pass.
All32 release diagnostic checks pass separately. The dedicated image_plan ASan
campaign completes11705 runs in46 seconds, final observed RSS177 MiB, under
512-MiB guard and16384-byte maximum input. Its bounded generated histories and
frozen/raw corpus exercise parser admission/replay; the short campaign is not a
whole-format exhaustiveness or allocator-quota claim. Quarantine is64 MiB with
256-KiB thread-local quarantine; corpus/output remain outside Git.

The first new nested-counter fixture expected rejection after writing an unchanged
zero retirement count. Its failing assertion is not counted as green; the fixture
now skips unchanged fields. One fuzz-target lint was repaired before the final
strict pass. No engine defect or successful recovery is inferred from those
intermediate failures.

Complete serialized EBIP-1 input is capped at9770208 bytes including metadata;
preflight scans all nested records before allocating owned image vectors.
Decoded-state, retained-view, simultaneous buffer, worker and process memory gates
remain open, as does ADR0031's single durable decision. No runtime WAL version,
backup format, managed write/ACK or real-data permission changes.

The logical block adds1086 and removes2 Rust lines (net1084), giving49779 physical
Rust lines (47543 excluding blanks/comment-only lines),1207 SDK and718 Python,
or51704 source lines. Documentation/configuration/locks/build output are excluded.


## Retained serialized payload admission (2026-10-06)

Complete stable1.99.0 and minimum Rust1.89.0 workspace runs pass702 tests each,
with17 ignored helpers and no failures. Five private cleanup/arithmetic/weak-ledger
cases and thirteen integration cases include exact bytes/one-byte-short, mixed
sizes, real4096 retained copies, source independence, admitted preparation and
writer lifetime. Two barrier-coordinated eight-thread cases hold winners while
observing exact object/byte usage. An independent64-case sequence predicts every
live payload/object charge and verifies each retained envelope against the base.
Six maximal permits fit64 MiB and a seventh refuses before allocation; no six
maximum heap workloads are allocated by that arithmetic test.

Both format checks, strict workspace/fuzz/optional-feature Clippy, locked build,
minimum fuzz bins and optional-feature compatibility pass. All32 release diagnostic
checks pass separately. The independent envelope_admission ASan campaign completes
25854 runs in46 seconds, final observed RSS155 MiB, with512-MiB guard and512-byte
maximum input. Generated histories have at most64 actions; the short corpus's
observed growth limit is20 bytes, so this is not exhaustive512-byte coverage.
Quarantine is64 MiB plus256-KiB thread-local quarantine. Corpus/output stay private.

EnvelopePool reserves complete serialized payload lengths and object slots before
encoded output allocation, including fallible explicit clones. Failed encoding,
unwinding and poisoned cleanup release exactly once. Borrowed full preflight
precedes copying external encoded bytes. Admitted preparation serializes without
exporting raw state or releasing its active writer early. Raw physical plans,
source/caller copies, decoded/retained models, caches, transient scratch, capacity
rounding, OS stacks and whole-process/server/WAL admission remain outside this
quota. No managed format, commit/ACK behavior or production gate changes.

The logical block adds950 and removes3 Rust lines(net947), giving50726 physical
Rust lines(48427 excluding blanks/comment-only lines),1207 SDK and718 Python,
or52651 source lines. Documentation/configuration/locks/build output are excluded.


## Streamed index snapshot and delta admission (2026-10-06)

Complete stable1.99.0 and minimum Rust1.89.0 workspace runs pass713 tests each,
with17 ignored helpers and no failures. All33 optional release diagnostics pass
on both toolchains, including a separate native allocator test with full10000-key
256-byte-text fixtures and four real scoped workers. Strict workspace/fuzz/feature
Clippy, both formats, minimum fuzz bins and locked workspace build pass.

Before index changes, the isolated full-capacity test fails with a7502704-byte
validation peak. Streaming complete admission reduces its observed validation,
fingerprint and no-op/one-page delta peaks to25376 requested bytes. Encode/decode/
apply retain only their necessary output/candidate and bounded scratch; one native
sample gives3154096/3308400/3307848 bytes. Four independent retained fixtures are
built outside profiling; four actual workers observe96360 transient bytes. All
operation-local current bytes return to zero in that run. These workload guards
exclude retained fixtures, stacks and profiler/allocator overhead and are not
whole-process limits. The first compile fixtures required two pointer-result/borrow
corrections and a usize counter correction before executing; only subsequent
completed runs are counted.

Six private cases include64 generated arena/corruption histories against the old
materialized-import oracle. Five integration cases include48 independent mixed-key
histories, sparse collapse/reuse, exact-base/terminal revision and byte preservation.
The manually packed1024-page arena reaches physical capacity with6678 rows and
one changed page. Original frozen format hashes and complete model/envelope
capacity checks still pass; managed file/WAL/backup/ACK formats remain unchanged.

Generated index_delta_sequences ASan completes23894 runs in46 seconds, RSS161MiB,
512-MiB guard/512-byte input cap. Its observed corpus growth limit is43 bytes.
Raw/CRC-repaired index_snapshot ASan completes1664931 runs in46 seconds, RSS156MiB,
512-MiB guard/69632-byte cap; observed growth limit24473 bytes. The raw seed is an
independent struct/zlib8192-byte empty snapshot matching the frozen SHA. Both use
64-MiB quarantine and256-KiB thread-local quarantine; short runs are not exhaustive
or hardware failure evidence. Corpus/logs remain outside Git.

The distinct coherent optimization block adds762 and removes40 Rust lines(net722),
giving51448 physical Rust lines(49116 excluding blanks/comment-only lines),1207 SDK
and718 Python, or53373 source lines. Documentation/configuration/locks/build output
are excluded. It is committed at the complete tested boundary without padding.


## Shared immutable index page ownership (2026-10-06/07)

Completed stable1.99.0 and minimum Rust1.89.0 workspace runs pass728 tests each,
with17 ignored helpers and no failures. Separately34 optional release diagnostic
checks pass on both toolchains. Both formatting checks, strict workspace/fuzz/
feature Clippy, minimum fuzz bins and locked workspace build pass. An interrupted
compilation was restarted; only the later complete runs count as successful.

Ten private arena cases observe actual page/key addresses, unchanged-owner
preservation, one-leaf replacement, splits/merges/root collapse, stable ID reuse,
dense remapping, Weak last-owner release, exhausted-arena refusal and four actual
thread writers. The independent48-case mixed-key model preserves up to eight
historical snapshots through accepted/discarded mutations and full replay. Its
initial text fixture accidentally exceeded256 bytes with its embedded NUL and
correctly received KeySize; reducing that fixture to255 bytes made the intended
valid-key history execute. Four model integration cases cover root-only EBIP
replay, one-page row-pointer change, stale predecessor refusal and four scoped
workers retaining valid output after source/plan release.

Before ownership changes, the isolated clone regression fails at3282472 requested
bytes. The new full10000-key256-byte-text/768-page fixture observes clone26304,
apply55912 and decode3285872 bytes. Validate/hash/delta remain25376; encode3154096
includes its3149824 output. Four real workers over separately decoded fixtures
observe95704 bytes; all operation-local current bytes return to zero. Clone/apply
now use128-KiB transient guards. These measurements exclude retained fixtures,
OS stacks, profiler bookkeeping and allocator overhead; they are not heap quotas.

Three strict-decoded preserved reports use the same four-project/10000-row/768-byte
index-replay config with actual parallel workers and retained old views. Their
index sources are pre-stream6ae2ebf, post-streamb9237d5 and the current shared-page
source hashes. Old post-stream source binding was checked against its published
commit before preserving the output. Requested peaks are196255216/172314032/
163075408 bytes; cumulative allocation is144137749368/142777335992/10046597208.
The1016-byte/four-block final coordinator/runtime residue remains observable.
Instrumented elapsed times and RSS are preserved separately and make no throughput
or production capacity claim. Raw heap traces and host paths are excluded.

The extended index_delta_sequences ASan campaign completes26001 runs in46 seconds,
final observed RSS154MiB,512-MiB guard and512-byte input cap. Up to four historical
views retain independent expected rows and exact bytes after current-state release.
Its observed corpus growth limit is29 bytes. The image_plan ASan campaign completes
41560 runs in46 seconds, RSS169MiB,512-MiB guard and16384-byte input cap; observed
growth limit33 bytes. Both use64-MiB quarantine and256-KiB thread-local quarantine;
these short campaigns are not exhaustive. Corpus and logs remain private.

The logical block adds809 net Rust lines, giving52257 physical Rust lines(49890
excluding blanks/comment-only lines),1207 SDK and718 Python, or54182 source lines.
Documentation/configuration/locks/build output are excluded. Immutable sharing
reduces redundant decoded copies without changing frozen EBIF/EBIX/EBIP, runtime
WAL1/2, backups or durable ACKs. Numeric decoded/transient/worker admission and
the combined durable table-index writer remain open under ADR0031.


## Validated shared primary export (2026-10-07)

Complete stable1.99.0 and minimum Rust1.89.0 workspace runs pass743 tests each,
with17 ignored helpers and no failures. Separately35 optional release diagnostic
checks pass on both toolchains. Both formats, strict workspace/fuzz/feature Clippy,
minimum fuzz bins and locked workspace build pass. Public to_stable conversion
and relational export retain full topology/physical/live-pointer verification.

The native allocation regression executes before code changes and fails at7480176
requested peak bytes. The same warm10000-key256-byte-text/768-page operation now
observes51680 peak bytes,26894632 cumulatively requested bytes and142614 blocks,
with current bytes returning to zero. It includes exported map retention and a
second explicit coverage verification; complete fixtures/cold cache construction,
allocator/profiler overhead and stacks are excluded. It is not a runtime heap quota.

Seven private conversion cases verify actual page ownership, old import parity,
sparse holes, the exact last arena ID, source release, isolated dense/stable policy,
strict malformed identity/count/topology/physical refusal and48 generated mutation
histories. Six relational cases include maximum256-byte Unicode/NUL keys, long-key
fallback, obsolete pointers, source/external isolation, four actual thread writers
and32 generated retained projection histories. Two additional automatic rebuild
cases verify root-only and changed-leaf prepare/physical-plan/replay through the
actual model rebuild path. Frozen bytes and full model/envelope capacities remain.

A fourth strictly decoded same-config four-project parallel index-replay report
retains the unchanged3334 history/792 index pages per project and root-only physical
plans. Its requested peak is139975354 bytes and cumulative allocation8945735192;
source hashes bind the changed index/projection implementation. It retains the
previously investigated1016-byte/four-block coordinator/runtime residue. Instrumented
elapsed35.74 seconds and RSS191952 KiB are observations, not throughput/capacity.

The extended index_operations ASan run first replays247 retained seeds:248 executions
complete in195 seconds, RSS206MiB. Initial corpus admission consumes the requested
45-second mutation window, so that run claims seed replay only. A separate fresh
corpus with two bounded synthetic seeds completes7960 executions in46 seconds,
RSS198MiB,512-MiB guard and2048-byte input cap; observed growth limit121 bytes.
Generated views match the original complete import and retain up to four old
snapshots through dense mutations/imports. These campaigns are not exhaustive.

The extended primary_lookup ASan run starts from a16704-byte synthetic committed
WAL generated by the actual CLI, with a256-byte short text key and a long excluded
key. Raw/CRC-repaired recovery then exports and verifies every admitted eligible
pointer without changing relational bytes. It completes1133103 runs in46 seconds,
RSS186MiB,512-MiB guard and20000-byte input cap. All campaigns use64-MiB quarantine
and256-KiB thread-local quarantine. Synthetic database/corpus/logs stay outside Git;
no actual user data or secrets are used or published.

The logical block adds764 net Rust lines, giving53021 physical Rust lines(50621
excluding blanks/comment-only lines),1207 SDK and718 Python, or54946 source lines.
Documentation/configuration/locks/build output are excluded. Numeric decoded plan/
model/cache/transient/worker reservation, complete durable writer and production
acceptance remain open. Runtime file/WAL/backup/ACK meanings are unchanged.

## Retained decoded vector admission (2026-10-07)

Complete stable 1.99.0 and minimum Rust 1.89.0 workspace runs each pass 760 tests,
with 17 ignored helpers and no failures. Separately, 36 optional release diagnostic
checks pass on both toolchains. Both formatting checks, strict workspace/fuzz/
feature Clippy, minimum-toolchain fuzz bins and locked workspace build pass.

Five private cases cover unexpected spare capacity, poison, unwind, actual Weak
last-owner release and maximal checked arithmetic permits. Twelve integration
cases cover explicit/disabled limits, exact byte boundaries, pointer identity,
independent copies, source lifetimes, malformed input and wrong/stale replay bases.
The object-cap case retains 4096 actual independently decoded owners. Two actual
eight-thread races hold every successful reservation through coordinated checks;
an independent 64-case property model distinguishes physical owners from handles.
Maximal arithmetic checks do not claim actual simultaneous maximal heap workloads.

The isolated optional native fixture has 256 history and 41 primary images. Its
reserved vector payload is 1228576 bytes; requested live/peak bytes are 1228776.
The additional 200 owner/control bytes are excluded from vector admission. Four
cloned handles allocate no additional bytes and retain one reservation. Last-owner
drop returns operation-local requested current bytes to zero. Disabled admission
observes 3292 transient bytes before refusal and no owned image vectors, guarded
against a regression to full materialization before quota checks. Fixtures,
source, pools, model construction, stacks, profiler and allocator overhead are
excluded; this is not a process heap/RSS quota or a cold replay observation.

The dedicated decoded_admission ASan campaign completes 33336 executions in 46
seconds with observed final RSS 163 MiB, a 512-MiB guard and 512-byte input cap.
Its independent owner/handle model checks exact charges, corruption refusal,
sharing/copies/release and complete replay. Observed corpus growth limit is 25
bytes. ASan quarantine is 64 MiB with a 256-KiB thread-local quarantine. The short
campaign is not exhaustive; synthetic corpus/logs remain outside Git.

Two initial test-fixture compilation errors (a captured model moved into a thread
iterator and a usize/u64 diagnostic comparison) were corrected before these
complete successful runs. They are not counted as passing checks.

The logical block adds 987 net Rust lines, giving 54008 physical Rust lines (51545
excluding blanks/comment-only lines), 1207 SDK and 718 Python, or 55933 source
lines. Documentation/configuration/locks/build output are excluded. Serialized
buffers and model lifetimes keep independent accounting. Model/cache/staging/
transient/worker reservation and the complete durable writer remain open.

## Admitted destination replay lifetimes (2026-10-07)

Complete stable 1.99.0 and minimum Rust 1.89.0 workspace runs each pass 775 tests,
with 17 ignored helpers and no failures. Separately, 37 optional release diagnostic
checks pass on both toolchains. Both formats, strict workspace/fuzz/feature Clippy,
minimum fuzz bins and locked workspace build pass. Runtime WAL/file/backup bytes
and acknowledgment behavior are unchanged; no durable gate is closed.

Four private tests observe actual reconstructed state/base/namespace owner Weak
lifetimes, exact-instance publication, discard, caller unwind and poisoned ledger
cleanup. Eleven integration cases check source independence, stale/foreign bases,
equal-state cross-pool publication refusal, retained-generation exhaustion,
disabled writer, stage/replay exclusion, Unicode/long-key row changes and retired
roots. A 48-case independent row model checks publication/discard with eight old
views. Eight actual synchronized threads hold exactly two accepted replay results
while checking the complete writer/generation ledger; all permits later release.

The optional native operation builds retained models/source/pools before profiling.
Generation exhaustion and existing project writer each observe zero requested
allocation before reconstruction. Accepted 256-row/3072-byte-value replay observes
1725668 retained requested bytes and 2521808 peak bytes; discard returns operation
current bytes to zero. Decoded input, retained fixture, stacks and profiler/
allocator overhead are excluded. This is count admission and a fixture observation,
not numeric model/cache/staging/transient or whole-process heap reservation.

The independent admitted_replay ASan sequence target completes 17932 executions
in 46 seconds with observed final RSS 159 MiB, a 512-MiB guard and 512-byte input
cap. Its owner-generation model compares old-reader transactions, pending replay,
held stages, foreign publication and exact complete row state. Observed corpus
growth limit is 28 bytes. ASan quarantine is 64 MiB with 256-KiB thread-local
quarantine. This short campaign is not exhaustive; corpus/logs remain private.

An initial fixture used a nonexistent Default implementation for AdmissionUsage;
explicit zero counters fixed its compilation. Two initial runtime test fixtures
had insufficient reader slots for their later assertions and correctly received
Admission(Readers). Corrected limits pass the complete subsequent runs above.

The logical block adds 989 net Rust lines, giving 54997 physical Rust lines (52499
excluding blanks/comment-only lines), 1207 SDK and 718 Python, or 56922 source
lines. Documents/configuration/locks/build output are excluded. Decoded vector
charges remain separate; model/cache/staging/transient budgets and the combined
durable writer remain open.


## Compact retained relational payload (2026-10-07)

The original public schema/row regressions and the isolated native retained guard
were run before the fix and failed. A five-byte schema name retained 131072 bytes;
a three-value row retained 1024 element slots. Native requested live/peak bytes
were 2132013/2132237. After private validated compaction, the same operation observes
2195 retained/2130282 peak bytes and zero current bytes after snapshot release.
Its input construction is included in the peak; this is a retained-shape guard,
not an input/model/cache/transient or allocator/RSS quota.

Five private cases preserve all value kinds, ETBL bytes, negative-zero bits,
maximum Unicode text deletes, empty payloads and actual repeated fast-path buffer
identity. Seven public cases cover insert/replace, exact history and pointer parity,
old readers, atomic refusal, direct file reopen, 64 columns and 3072-byte text keys.
A 48-case independent history model compares accepted/refused rows and canonical
page fingerprints. Two managed cases execute commit/rollback/abort, exact WAL
preservation, recovery/checkpoint/compaction and retained readers in WAL 1/2.

The retained_payload ASan target completes 44400 executions in 46 seconds, with
observed final RSS 119 MiB, a 512-MiB guard and 512-byte input cap. Its independent
row model inflates owned input capacities, checks retained shapes, exact physical
fingerprints/current pointers, old views and invalid-input refusal. Observed
corpus growth limit is 53 bytes. ASan quarantine is 64 MiB with a 256-KiB thread-local
quarantine. This short campaign is not exhaustive; corpus/logs stay private.

Both optional release diagnostic suites pass 38 checks. Initial captured-output
runs observed 36 bytes retained by test-output logging inside the measured region;
moving diagnostic output after profiler shutdown makes both complete suites pass
without weakening the zero-current assertion. The standalone regression still
measures the same engine/input allocation boundary and counters.

Complete stable 1.99.0 and minimum Rust 1.89.0 workspace runs each pass 789 main
tests with 17 ignored helpers and no failures.
Both formats, strict workspace/fuzz/feature Clippy, minimum fuzz bins and locked
workspace build pass. No durable-index, hardware power-loss or production gate closes.

The logical block adds 920 net Rust lines, giving 55917 physical Rust lines (53399
excluding blanks/comment-only lines), 1207 SDK and 718 Python, or 57842 source
lines. Documents/configuration/locks/build output are excluded. Runtime formats,
validation/refusal meaning and existing durable ACK rules remain unchanged.


## Compact retained index buffers (2026-10-07)

Three private constructor/insertion regressions and a native retained guard ran
before implementation and failed. One three-byte UTF-8/NUL key backed by a
1-MiB incoming String retained 1049016 requested bytes after insertion. After
validated publication compaction, the same sample retains 323, peak 1048928;
separate leaf/branch samples retain 43 each, peaks 1089696/1081344. All three
operation-local current counters return to zero after release. Large input is
constructed inside each profiler; the empty fixture and old empty view are outside.
These are requested-payload observations, not allocator usable-size/RSS quotas.

Thirteen private checks cover exact actual vector/text capacities, empty/maximal
text, all branch arities, typed refusals, fast-path addresses, both arena policies,
split/borrow/merge/root collapse, retired ID reuse, old-owner release, the full
10000-entry/768-page arena and delta/import/snapshot parity. Their 48-case
independent mutation model compares complete rows, exact images, equal-page
owners and four retained views. Four public cases inspect borrowed keys before
serialization, preserve Unicode distinctions/failed-operation addresses and
execute standalone full/delta publication with repeated reopen. Frozen EBIX-1
wire compatibility remains covered by the existing published-image regression.

The final index_retained ASan campaign completes 4250 executions in 46 seconds,
observed final RSS 267 MiB, 512-MiB guard and 512-byte input cap. Corpus includes
synthetic split/merge/mixed-operation seeds; observed growth limit is 323 bytes.
It inflates owned inputs, checks borrowed key capacities before any round trip,
compares an independent row map, complete views and retained historical bytes.
Quarantine is 64 MiB plus 256-KiB thread-local quarantine. An earlier equivalent
pre-lint-syntax campaign completed 4517 runs; final source was rerun after lint
cleanup. These short campaigns are not exhaustive; corpus/logs stay private.

Complete stable 1.99.0 and minimum 1.89.0 workspace runs each pass 806 main tests
with 17 ignored helpers and no failures. Both optional release suites pass 39
diagnostic checks. Workspace/fuzz format checks, strict workspace/fuzz/optional
Clippy, minimum fuzz-bin compatibility and locked workspace build pass.

The logical block adds 948 net Rust lines: 56865 physical Rust lines, 54314 without
blanks/comment-only lines, plus 1207 SDK and 718 Python, total 58790 source lines.
Documents/configuration/locks/build output are excluded. Numeric model/cache/
staging/transient budgets, combined durable-index publication and production
acceptance remain open.


## Primary-key inner join probes (2026-10-07)

The valid 400-row-per-side regression fails against the preceding published
implementation with Limit("query work"), then passes with primary probes. Its
initial fixture used an incorrect event variant and was corrected before that
baseline run. Three private checks inspect actual executor counters: complete
200-by-200 equality output needs 600 rather than 80000 logical visits; output
charges are equal. Null/missing probes and every ON/WHERE node still charge the
original shared work budget and exhaustion is refused.

Eleven public cases cover reversed necessary AND equality, full ON/WHERE, aliases,
nonzero primary positions, star labels, nullable/missing/repeated foreign keys,
ordering/limits, fallback OR/NOT/non-primary/same-side/literal conditions, complete
empty/LIMIT0 binding/type checks, UTF-8/NUL/255/256/257/3072-byte keys and old views.
The 64-case independent map compares complete many-to-one output and fallback
parity. Sorted joins still refuse intermediate data above 8 MiB before LIMIT 1.
Managed WAL 1/2 exercise staged reads, rollback with exact WAL preservation,
commit, old readers, checkpoint/compaction, reopen, verified backup/restore and
independent subsequent writes. Existing old-plan EXPLAIN assertions were updated
for eligible queries after they failed in the first full run; subsequent complete
runs below are green. Typed validation and fallback assertions remain active.

A release-native warmed sample holds 4000 rows per side, 3072 hidden bytes per
row and LIMIT2. The equivalent OR FALSE predicate selects the original fallback
in the same source. Requested peaks are 25550648 versus 14328 bytes; both retain
348 output bytes and return to zero after result release. Complete fixture/cache
construction is outside the measured region, as are allocator overhead/rounding,
stacks and profiler bookkeeping. Source-bound counters describe operation-local
requested allocations, not cold memory, throughput, RSS or a process heap quota.

The sql_primary_joins ASan campaign completes 16539 executions in 46 seconds,
final observed RSS 199 MiB, a 512-MiB guard/512-byte input cap, growth limit146.
Synthetic generated nullable integer/text inputs include excluded long keys,
reversed equality, parameter filters, ordering/LIMIT, independent map results,
fallback parity and old physical fingerprints. Quarantine is 64 MiB plus 256-KiB
thread-local quarantine. The short campaign is not exhaustive; corpus/logs stay
private. Unknown strict client access enums remain refused.

Stable 1.99.0 and minimum Rust 1.89.0 final complete workspace runs each pass 820
main tests, 17 ignored helpers, zero failures. Both optional release suites pass
40 diagnostic checks. Workspace/fuzz format, strict workspace/fuzz/optional Clippy,
minimum fuzz bins and locked workspace build pass. The updated SDK passes 11
unit tests and seven actual HTTP/restart checks including new EXPLAIN/SELECT
calls. OpenAPI declares primary_join without changing the previous field set.

The block adds 1057 net Rust and 26 SDK lines, 1083 total source lines. Current
physical Rust is 57922 (55334 excluding blanks/comment-only lines), SDK1233,
Python718, total59873 source lines. Documents/configuration/lockfiles/build output
are excluded. Stored formats, durable ACK rules and production gates are unchanged.


## Ordered, bounded primary joins (2026-10-07)

The valid 1500-by-1500 wide-row regression fails ORDER BY left.id DESC LIMIT 2
against published 2719a6a with Limit("intermediate rows/bytes"). After early source
termination fixes it, a second narrow 1500-ID regression still fails because full
hidden joined fields are retained; projected streaming fixes that separately.
Both baseline failures were executed before their respective implementation fixes.
The implicit read-script fixture originally asserted an incorrect committed flag;
it was corrected to the existing report contract. Exact WAL bytes remain unchanged.

Nine public checks cover both directions/secondary null keys, many-to-one match
counting, necessary points/ranges/reversed comparisons, OR/NOT refusal to infer
bounds, right/non-primary stable sorting, full intermediate-byte refusal for those
orders, mixed255/256/257/3072-byte UTF-8/NUL keys, long-key physical paths and old
views, integer extremes, typed empty/LIMIT0 validation and full 10000-row capacity.
A 64-case independent map compares output and the equivalent original fallback.
Both managed WAL versions execute 64 successive ordered reads, preserve exact WAL
and checkpoint/reopen data. Four additional private checks inspect actual work:
two ordered matches: 6 visits, left point: 4, two range matches: 12, contradiction: 0;
non-primary order still visits all candidates, false filters/output caps still refuse.

The release-native warmed 1500-row-per-side/3072 hidden-byte fixture observes
requested peaks 8411/8769/8222 for ordered/range/point LIMIT 2, retained 348/348/284
and zero after release. Exact source hashes appear in the measurement artifact.
Fixture/both caches precede profiling; cold construction, allocator overhead/rounding,
stacks and profiler bookkeeping are excluded. This is operation-local requested
allocation evidence, not cold memory, throughput, RSS or a whole-process quota.

The ordered_primary_joins ASan smoke completes 35480 executions in 46 seconds,
observed RSS 180 MiB, input cap 512 bytes / guard 512 MiB, final corpus growth limit 106 bytes.
Eight synthetic integer/text/point/range/direction seeds initialize the private
corpus. Generated null/missing keys, long UTF-8/NUL keys, independent ordered map
results, original fallback parity and historical fingerprints are checked.
Quarantine is 64 MiB with 256-KiB thread-local quarantine. This short run is not
exhaustive; raw logs and corpora are not published.

Complete stable 1.99.0 and minimum 1.89.0 runs each pass 833 main tests with 17 ignored
helpers, zero failures. Both optional release suites pass 41 diagnostics. Workspace/
fuzz formatting, strict workspace/fuzz/optional Clippy, minimum fuzz-bin check and
locked build pass. SDK passes 11 unit and 7 actual native HTTP/restart checks.
No SDK enum, API route, SQL grammar, storage bytes or durable ACK change.

The logical block adds 981/−21=960 net Rust lines: 58882 physical Rust, 56266 without
blank/comment-only lines, SDK 1233 and Python 718, total 60833 source lines. Documents,
configuration, lockfiles and generated build output are excluded. Numeric model/
cache/staging/transient budgets, combined durable writer and production gates remain open.


## Bounded stable primary-join sort (2026-10-07)

The valid wide 1500-by-1500 right-order LIMIT 2 regression fails against published
ef6b62e with the intermediate-byte limit, then passes with bounded selection.
The initial managed fixture used a nonexistent factory and incorrect recovery
import; it was corrected to existing create/explicit compact/recover APIs before
the complete green runs. No production API was added for that fixture.

Seven private checks inspect actual selected rows/counts/bytes: worst-root
replacement, stable ties, both directions and NULL positions, exact match-count
exhaustion, byte-growth/refused-replacement preservation, discarded large bodies
and smaller replacement release. Seven public checks cover all supported sort
types, finite float bits/signed zero, Unicode/NUL/bytes, secondary keys, exact
fallback parity, source points/ranges/full ON/WHERE, typed LIMIT0 validation,
retained-byte refusal, stable ties across deletion and old images. A 64-case
independent nullable many-to-one map checks ordering, filters and limits with
nonzero right primary position. Both managed WAL versions verify staged reads,
rollback, failed atomic scripts, exact WAL bytes, checkpoint/compact/reopen and
immutable older views. Existing large-limit refusal cases remain active.

The warmed release sample has 1500 rows per side and 3072 hidden bytes per row.
Right, left non-primary and equal-key LIMIT 2 samples observe requested peaks
21051/21053/21055, each retaining 444 bytes and releasing to zero. Exact source
hashes appear in the measurement artifact. Fixture/both caches precede profiling;
cold construction, allocator overhead/rounding, stacks and profiler data are
excluded. This measures operation-local allocations, not RSS/throughput/quota.

The limited_join_sort ASan smoke completes 17306 executions in 46 seconds,
observed RSS 171 MiB, input cap 512 bytes / guard 512 MiB and corpus growth limit 141 bytes.
Sixteen synthetic direction/NULL/source-bound/secondary-order modes seed the private
corpus. Independent rank maps, missing/null foreign keys, parameter filters,
stable ties, full original fallback parity and old physical fingerprints execute.
Quarantine is 64 MiB with 256-KiB thread-local quarantine. It is not exhaustive;
raw logs/corpora remain private. Parser/format code and stored versions are unchanged.

Complete stable 1.99.0 and minimum 1.89.0 workspace runs each pass 847 main tests,
17 ignored helpers, zero failures. Both opt-in release suites pass 42 diagnostics.
Workspace/fuzz formatting, strict workspace/fuzz/optional Clippy, minimum fuzz-bin
compatibility and locked build pass. SDK format and 11 unit tests pass; seven actual
HTTP/restart checks include a new parameterized right-order JOIN.

The logical block adds 940 net Rust and 12 SDK lines, 952 source lines. Current Rust
is 59822 physical/57171 excluding blank/comment-only lines, SDK 1245, Python 718;
total 61785 source lines. Documentation/configuration/locks/generated files are
excluded. Full numeric model/cache/staging/transient admission, combined durable
table/index writer and production gates remain open.


## Projected output admitted before copying (2026-10-07)

A valid warmed release primary-JOIN query with 1500 rows per side, 3072 hidden
bytes, 32 selected copies and LIMIT 1000 returned Limit("output bytes") after a
99388593-byte requested peak. The actual 32-MiB native guard failed against
published 80b51ef before implementation. After per-row borrowed admission and
incremental common projection, the same sample retains the same error, peaks at
14210993 and releases to zero. Final single-table/full-sort, primary-order stream
and fallback-JOIN peaks are 12889830/8355020/14189897, all refused with no retained
error allocation. Exact source hashes/fixture dimensions appear in the artifact.
Complete fixture/cache construction precedes profiling; allocator overhead/rounding,
stacks and profiler data are excluded. This is a warmed operation-local observation,
not cold memory, RSS, throughput or a whole-process/transient quota.

Four private checks inspect actual shared byte charges before returned output:
repeated selected cells, NULL/empty values, float bits, unchanged work, exact last
admission/next refusal and typed invalid-index refusal before charging. Six public
checks compare all read paths at their exact byte boundary, repeated/aliased/star
metadata, all types, float bits, full binding on empty/LIMIT0, previous snapshots
and an independent 64-case nullable projection model. An initial fixture requested
1500 explicit fields and failed the existing 64-column parser cap; it was corrected
to test that real cap without weakening the parser. Both managed WAL versions
preserve exact committed journal/state after a staged write and cumulative output
refusal. Rollback, checkpoint/reopen, verified backup/restore and independent copy
writes pass. Logical output formula, API, SQL grammar and stored bytes are unchanged.

The projection_budget ASan smoke completes 7881 executions in 46 seconds, observed
RSS 398 MiB, input cap 512 bytes / guard 512 MiB, corpus growth limit 307 bytes. Sixteen
synthetic wide/small projection and direction/null/order modes seed the private
corpus. Independent nullable selected-cell models compute the shared charge and
avoid constructing over-budget expected results; all read plans either agree with
exact rows or refuse at the original logical cap. Old physical fingerprints stay
unchanged. Quarantine is 64 MiB with 256-KiB thread-local quarantine. A small manual
modulo lint was fixed before the recorded final campaign. This run is not exhaustive;
logs/corpora remain private. Parser/format code is unchanged.

Final stable 1.99.0 and minimum 1.89.0 complete workspace runs each pass 857 main tests,
17 ignored helpers, no failures. Both optional release suites pass 43 diagnostics.
Workspace/fuzz formatting, strict workspace/fuzz/optional Clippy, minimum fuzz bins
and locked build pass. SDK 11 unit and 7 real HTTP/restart checks pass.

This focused repair adds 811/−32=779 net Rust lines: 60601 physical Rust, 57934 excluding
blank/comment-only lines, SDK 1245 and Python 718; total 62564 source lines. Docs, config,
lockfiles and generated files are excluded. Numeric model/cache/staging/transient
admission, combined durable writer and production acceptance remain open.


## Borrowed ordinary sorted sources (2026-10-07)

A valid 2800-row/3072-hidden-byte ordinary non-primary ORDER BY LIMIT 2 fails
with the original intermediate-byte refusal against published 72c97fa before
implementation. A separate warmed native 1500-row sample accepts the query but
fails its 128-KiB peak guard at 4842026 requested bytes. Both actual regressions
pass after borrowing physically checked source rows and retaining the best LIMIT
candidates. The final native sort/range/point requested peaks are 11634/11990/5196,
retaining 282/282/250 and releasing to zero. Fixtures and caches precede profiling.
Exact hashes/exclusions appear in the source-bound artifact; these are operation
observations, not cold memory, RSS, throughput or a process/transient quota.

Three private work checks establish 200 visits for 200 source rows despite LIMIT 2,
2 for a necessary point, 40 for ten range rows and their complete filter, zero for
contradictory/missing sources and 400 for a false filter. Work/output refusals remain.
Seven public checks cover all supported sort types, stable ties and secondary
orders, NULL placement, finite float bits, aliases/nonzero primary positions,
integer boundaries, mixed UTF-8/NUL 255/256/257/3072-byte primary keys, old views,
full empty/LIMIT0 binding and a 64-case independent nullable sorted-map model.
Large retained/output requests still fail at the original logical limits. Two
older wide-source refusal fixtures now request their actual large retained LIMIT;
their staged-write rollback and unchanged committed-WAL assertions still pass.
Both managed WAL versions preserve staged reads, explicit rollback, exact failed
script bytes, commit/checkpoint/reopen, verified restore and independent writes.

The limited_table_sort ASan smoke completes 43506 executions in 46 seconds,
observed RSS 188 MiB, input cap 512 / guard 512 MiB and corpus growth limit 151 bytes.
Thirty-two synthetic seeds exercise direction/null/integer-or-long-text primary
key/point/range/OR fallback modes. An independent last-wins nullable map restores
primary source order before its own stable rank sort; old physical fingerprints
stay unchanged. Quarantine is 64 MiB with 256-KiB thread-local quarantine. This is
a bounded smoke campaign; logs, corpora and raw allocator data remain private.

Final stable 1.99.0 and minimum 1.89.0 complete workspace runs each pass 867 main
tests, 17 ignored helpers and no failures. Both optional release suites pass 44
diagnostics. Workspace/fuzz formatting, strict workspace/fuzz/optional Clippy,
minimum fuzz-bin checks and locked build pass. SDK format/11 unit/7 real HTTP and
restart checks pass, including a parameterized non-primary NULL-first table sort
whose EXPLAIN keeps scan. One unused test import was removed before final lint.
Previous commit 72c97fa also has all five hosted CI jobs confirmed successful.

This block adds 912 net Rust and 12 SDK test lines: 61513 physical Rust, 58829
excluding blank/comment-only lines, SDK 1257 and Python 718; total 63488 source
lines. Docs/config/lockfiles/generated output are excluded. Stored formats, durable
ACK and SQL/API types are unchanged. Numeric model/cache/staging/transient budgets,
combined durable writer and production acceptance remain open.

## Borrowed predicate and retained-candidate admission (2026-10-07)

Against published 281c01d, a native warmed fixture with two 1500-row tables,
3072 hidden bytes and LIMIT 2 fails its actual total-allocation guards. Ordinary
sort/sorted-primary-JOIN/false-ON-primary-JOIN request 9819837/19937652/19937358
summed allocation bytes. After immutable row views and pre-copy winner admission,
the same samples request 5074173/10302324/10289358. Peaks are 11634/17980/5180;
live output 282/282/122, after-release zero. An early overly small 1-MiB guard was
revised to account for physical validation; the final 6/12-MiB guards also fail
before implementation and pass afterward. Exact fixture/source hashes are in
the observation artifact. Summed requests do not describe simultaneous memory,
throughput or RSS. Cold fixture/cache, allocator overhead/rounding, stacks and
profiler data are excluded. Physical validation, winning candidates and keys
still allocate; whole-memory/transient quota is not claimed.

Nine new private checks cover checked split offsets/usize::MAX, all value payload
costs/float bits, null/direction comparison parity, original three-valued predicate
results and exact node work, both Boolean branches under work exhaustion, repeated
joined projection at last-admitted/first-refused output bytes, typed invalid
indices, source ordinals, heap charges and growth/replacement refusal atomicity.
Two public checks join maximum 64-column schemas and all 128 star fields, final
primary positions, aliases/repeated right fields, original fallback parity, full
nullable predicates and retained old snapshots. Both WAL versions preserve staged
changes to both sources, rollback and a write followed by cumulative output
refusal with exact prior committed bytes. Checkpoint/reopen, verified backup/
restore and independent writes pass. An initial test fixture used nonexistent
snapshot/backup helper names; it was corrected to the existing view and backup
crate APIs before these successful runs. No engine behavior was changed for it.

The borrowed_candidates ASan smoke completes 50234 executions in 46 seconds,
observed RSS 174 MiB, input cap 512 / guard 512 MiB and corpus growth limit 102 bytes.
Thirty-two synthetic seeds exercise left/right sorts, both directions/NULL rules,
complete nullable ON/WHERE and missing right probes. An independent last-wins map
uses its own three-valued acceptance and stable rank order; original fallback
results and old physical fingerprints also agree. Quarantine is 64 MiB with
256-KiB thread-local quarantine. This bounded smoke is not an exhaustive audit;
raw logs/corpora remain private. A focused command initially used a nonexistent
projection test target; the correct projection_budget target passed afterward.

Final stable1.99.0/minimum1.89.0 complete workspace runs each pass878 main tests,
17 ignored helpers, no failures. Both optional release suites pass45 diagnostics.
Workspace/fuzz formatting, strict workspace/fuzz/optional Clippy, minimum fuzz
bins and locked build pass. SDK format/11 unit/7 actual HTTP/restart checks pass.
All five hosted jobs of preceding281c01d are confirmed successful.

The block adds879/removes65=814 net Rust lines. Totals:62327 physical Rust,
59632 excluding blank/comment-only lines,SDK1257,Python718,64302 combined source
lines. Docs/config/lockfiles/generated files are excluded. No stored format,
SQL/API/EXPLAIN enum, durable ACK or unsafe change. Numeric model/cache/staging/
transient admission, combined durable writer and production acceptance remain open.

## Shared borrowed physical-row codec (2026-10-07)

A real native regression against published5b416eb resolves1000 wide rows per
sample. Integer/wide-text and3072-byte text-primary samples request total3344000/
6416000 bytes, peaks3240/6208, retaining zero. Both original and final128-KiB guard
runs fail before implementation. Shared validated borrowed cells and direct
current-key comparison reduce both totals to104000 in1000 blocks, peak104, zero
retained. An initial16-KiB guard remains too small after repair because per-call
schema validation still allocates; the corrected bound was independently rerun
against the actual published baseline, then restored repaired sources and passed.
Exact hashes/dimensions/exclusions appear in the artifact. Cold fixtures/keys/
locations precede profiling; allocator overhead/rounding, stacks and profiler
data are excluded. Payload copy removal is not a whole-memory/transient quota.

Five new public codec checks cover all types, empty/count-mismatched rows, signed
zeros, exact3072-byte boundaries, every truncated prefix, versions/tags/nonfinite
floats/Boolean/UTF-8/oversized values and trailing errors after early mismatch.
Two128-case properties verify independent generated values and arbitrary complete
record validation. Three private ETBL checks preserve insert/replace/foreign
identity, all truncated prefixes, envelope failures, zero table ID and malformed
non-row payload refusal. The first private fixture passed slices to an unnecessarily
narrow Vec-reference helper; the helper was corrected to borrowed slices. A newly
unused import was removed before final strict lint. The original golden codecs,
stale locations, all-type replay/old-view/SQL/WAL/backup recovery tests pass.

The row_match ASan smoke completes3238754 executions in46 seconds, observed
RSS180 MiB, input cap4096/guard512 MiB, corpus growth limit4096 bytes. Six synthetic
seeds include empty/NULL/Boolean/text/binary records. Arbitrary and mutated records
compare matcher results/errors with complete owned decode; independent generated
fields verify exact matches/mismatches. This uses a shared reader and does not
claim an independent implementation of the file format. Existing frozen-byte and
explicit malformed-record checks supply that additional evidence. Quarantine is
64 MiB with256-KiB thread-local quarantine. Raw logs/corpora remain private.

Final stable1.99.0/minimum1.89.0 complete workspace runs pass886 main tests each,
17 ignored helpers and no failures. Both optional release suites pass46 diagnostics.
Workspace/fuzz formatting, strict workspace/fuzz/optional Clippy, minimum fuzz-bin
checks and locked build pass. SDK format/11 unit/7 actual HTTP/restart checks pass.
All five preceding5b416eb hosted CI jobs are confirmed successful.

This focused codec block adds463 net Rust lines:62790 physical Rust,60085 excluding
blank/comment-only lines,SDK1257,Python718;64765 combined source lines. Docs/config/
locks/generated output are excluded. No EROW/ETBL/page/WAL/cache bytes, format
versions, SQL/API/EXPLAIN, durability ACK or unsafe change. Numeric model/cache/
staging/transient admission, combined durable writer and production gates remain.

## Bounded stack schema validation (2026-10-07)

A native zero-temporary-heap regression fails against published46317ba.1000 paired
schema/key checks request208000 bytes for1/2 columns and2272000 for64 columns,
in2000/2000/20000 blocks, peaks104/104/1136 and live zero. After bounded borrowed
name/position arrays, every sample requests zero bytes/blocks/peak/live. The two
preceding1000-call physical-resolution samples also request zero. Exact current
hashes/dimensions/exclusions appear in the artifact. Fixtures/names/keys precede
profiling; stacks, allocator rounding/overhead and profiler data are excluded.
This proves the measured temporary heap removal, not throughput, total stack
use, cold memory, RSS or a whole-process/transient quota. All schema rules remain.

Four private checks preserve original identifier/duplicate/primary/count error
precedence, all2016 duplicate-position pairs in64-column maximum-name schemas,
count65 refusal, oversized later names and a256-case independent sequential-set
model. Oversized names use placeholders before comparisons and still refuse at
the original logical position. Inputs remain immutable; type/null/value/key
checks execute. Schema/API/file/cache/WAL bytes and durable ACK are unchanged.

The final schema_validation ASan smoke completes1524137 executions in46 seconds,
observed RSS185 MiB, input cap512/guard512 MiB, corpus growth limit512 bytes.
Thirty-three synthetic seeds cover empty/valid/over-count schemas, invalid table/
column names, duplicates,63/64-byte names, primary boundaries/types/nullability.
An independent sequential-set validator agrees with exact error categories and
original input. Quarantine is64 MiB with256-KiB thread-local quarantine. The prior
1247622-run smoke binds the earlier name preflight and is not relabeled as final.
This is a bounded campaign; raw logs/corpora remain private.

The first stable workspace run passes889 main tests but fails seven rustdoc
compile targets with E0460/E0463 after code/metadata changed during the run; it is
not a complete green result. The final oversized-name guard and fourth check were
then frozen. Complete reruns on stable1.99.0/minimum1.89.0 each pass890 main tests,
17 ignored helpers and no failures, including documentation compilation. Both
final optional release suites pass47 diagnostics. Workspace/fuzz formatting,
strict workspace/fuzz/optional Clippy, minimum fuzz bins and locked build pass.
SDK format/11 unit/7 actual HTTP/restart checks pass. All five preceding46317ba
hosted jobs are confirmed successful. No accepted-behavior regression was inferred
from the intermediate metadata mismatch; final source was rechecked completely.

The block adds279/removes5=274 net Rust lines. Totals:63064 physical Rust,
60354 excluding blank/comment-only lines,SDK1257,Python718;65039 combined source
lines. Docs/config/locks/generated output are excluded. Numeric model/cache/
staging/transient admission, combined durable writer and production gates remain.


## 2026-10-07 — complete borrowed schema inventory

A native regression first fails against881ca75: preparing one prebuilt index
candidate with1/64/128 tables, each64 columns and zero rows, requests
12018/729648/1459120 allocation bytes with peaks6181/361624/723160. The repaired
source requests792/11184/22192, peaks656/2480/4784, live656/2480/4784 while the
prepared state is held and zero after drop. A separate1000-pass scan of all128
borrowed schemas/counts requests zero bytes/blocks. Cold fixtures/stage creation,
allocator overhead/rounding/stacks/profiler data precede or sit outside tracking.
See the [source-bound artifact](measurements/2026-10-07-borrowed-schema-inventory/operation-allocations.json)
and [ADR0065](adr/0065-borrowed-schema-inventory.md); these are allocation observations,
not throughput, cold memory, RSS, model admission or whole-process quotas.

Two parallel diagnostic tests initially contaminated a process-wide allocator
with the other fixture and test-harness teardown. The default parallel command
actually fails; a fixture mutex alone also fails. Both samples now execute in
one test lifetime, and the ordinary release command passes without a serial
test-runner override. The first stable workspace build is killed with exit137
during concurrent compilation, and is not counted as a pass. The complete stable
rerun limits build jobs to2. Runtime source stays frozen through final suites.

Stable1.99.0 and minimum1.89.0 each complete900 main tests,17 ignored process
helpers, zero failures, and all doc tests. Each optional release suite passes48
diagnostics. Workspace/fuzz formatting, strict workspace/optional/fuzz Clippy
and minimum fuzz-bin compilation pass. New catalog-model property sequences
use128 cases; deterministic tests cover128-by-64 inventories, ID gaps/recreation,
front/back lengths and exhaustion, exact pointer identity/owned copies, old COW
views, concurrent readers, last-table missing roots, unchanged roots and exact
image-plan replay fingerprints. Both WAL versions preserve metadata through
failed/rolled-back DDL, reopen and verified backup restore. Wide cache warmup
preserves malformed optional bytes, immutable schemas and WAL bytes. Separate
project status remains scoped across failed DDL and restart.

Final bounded ASan schema_inventory fuzzing executes4396 inputs in55 seconds,
RSS180 MB/input256/growth256/guard512 MB, starting from the accumulated corpus
after33 deterministic originals. It compares an independent live catalog across
create/drop, duplicate/missing refusal, COW row mutation, discarded candidates,
old views and full physical replay. The earlier23945-input campaign predates a
Clippy-required chunk iterator change and is not substituted for the final run.
The harness uses bounded32 commands/eight names/1..64 columns, and no unsafe.

Physical source totals:63845 Rust,61104 excluding blank/comment-only lines,
SDK1257 (455 TypeScript+802 test MJS), Python718;65820 total source lines.
The logical block adds781 net Rust lines, no SDK/Python changes. Numeric model/
cache/staging/transient budgets, the combined durable writer and production
acceptance gates stay open. Mandatory WAL/file/cache bytes and HTTP/SQL meanings
are unchanged; borrowed metadata adds a Rust API without removing owned access.

The final locked workspace build passes. SDK formatting/strict compilation and
11 unit tests pass; seven actual native HTTP/restart checks pass against the
newly built original server. Previous published881ca75 hosted CI completes all
five jobs successfully; this block's hosted CI is evaluated separately after push.


## Hosted allocation runner follow-up

Hosted da65a95 run37634294902 completes four jobs successfully, while the
allocation job fails in the older schema_validation sample: its process-wide
allocator observes900 live bytes from the surrounding test execution. The pure
stack validation path is unchanged. Focused d2bab9c makes allocation diagnostics
use explicit `--test-threads=1`, also isolating libtest lifecycle allocations.
All43 published integration diagnostic targets pass locally with published query
sources and that option; the other five library checks passed in the preceding
full48-check suite. Pending nested-join sources are restored byte-for-byte after
that separate baseline check. The runner repair changes no engine or schema
format. Its hosted result is tracked separately.


## 2026-10-07 — borrowed nested-loop sources

A real native regression first fails on da65a95. With100 rows per source and
3000-byte hidden text, false ON/WHERE summed requests are64469242/64468894,
peaks632072/631720, live122/drop0;100-match sorted requests64475392, peak944680,
live282/drop0. Repaired source requests9530/9182/634880, peaks3960/3608/626040,
live122/122/282, all zero after result drop. The cold fixture, derived indexes and
fingerprints precede tracking. The existing4000-row-per-source two-match comparison
now measures fallback peak80088 and primary-probe peak2622. Its obsolete >16 MiB
waste requirement is replaced with a256 KiB fallback upper guard; older artifacts
retain their original source hashes. [Observations](measurements/2026-10-07-borrowed-nested-join/operation-allocations.json)
and [ADR0066](adr/0066-borrowed-nested-loop-sources.md) record scope/exclusions. These
are summed operation allocations, not throughput, RSS, cold heap, total stack
use, retained-model admission or a whole-process budget.

The first new work assertion incorrectly assumes public point/range extraction
for fallback joins. Inspection of the actual planner confirms full-source access
is retained; corrected assertions verify23/33/33 work for those public left
filters. Separate internally bound test plans verify8/21/0 without altering the
planner. Remaining exact work/Boolean/output charge checks pass. Complete stable
1.99.0/minimum1.89.0 suites each pass909 main tests,17 ignored process helpers,
zero failures and all doc tests; each optional serial release suite passes49
diagnostics. Workspace/optional/fuzz strict Clippy and formatting pass.

Independent128-case nullable pair models cover OR/NOT, stable multiple sort keys,
NULL direction, repeated fields, parameters, UTF-8/NUL, bytes and signed-zero
bits. Real byte/row/work refusals still hold with tiny sorted LIMIT and narrow
output. Both WAL versions preserve staged joins, late failure and explicit
rollback, old views, restart and verified backup restore. Long text keys through
3072 bytes, self joins, missing values, contradictions and binding at LIMIT0 pass.
Final bounded ASan nested_join fuzzing executes86507 inputs in46 seconds, RSS185 MB,
input512/growth151/guard512 MB,33 initial seeds. It compares independent cross-pair
models with NULL truth, flag branches, hidden text/bytes, stable ties, equivalent
OR-FALSE predicates and actual bounded_nested_loop EXPLAIN. No unsafe is added.

Source totals:64661 physical Rust,61894 without blank/comment-only lines, SDK1257
(455 TypeScript/802 MJS), Python718;66636 total source lines. Net Rust growth816;
no SDK/Python change. Numeric model/cache/staging/transient limits and the combined
durable writer remain open. SQL/HTTP/file/WAL/cache meanings and durable ACK stay
unchanged. Hosted d2bab9c runner repair completes all five jobs successfully; the
new engine block's hosted result is checked independently after its own push.

Minimum Rust compiles all locked fuzz bins. The locked workspace build, SDK
formatting/strict compilation,11 unit tests and seven actual native HTTP/restart
checks pass against the newly built original server. All runtime sources remain
frozen during the final workspace suites and these publication checks.
## Bounded password verifier foundation, 2026-10-07

The auth helper has 17 new unit/property cases alongside the three existing key
cases, plus two synthetic original-engine persistence integration tests. Four
512-case generated suites independently check raw record acceptance, opaque
payload round trips, exact cost refusal and lease lifetime counts. Tests also
exercise shared races, four actually held workspaces, concurrent real hashing,
unwind release, explicit block wiping before deallocation, byte boundaries,
Unicode/NUL, no truncation/normalization, redacted Debug and full header mutations.
No test reads freed memory or introduces unsafe code.

Three raw C Argon2 vectors match RustCrypto at identical explicit parameters.
The optional pinned [oracle script](../tests/password_oracle.py) reproduces the
committed artifact exactly; CI repeats that independent comparison in an isolated
test environment. The Rust runtime does not depend on that Python/C oracle.
Both WAL versions preserve acknowledged digest replacements, exclude rolled-back
ones, preserve old views and restore exact bytes from independently inspected
backups. Synthetic plaintext inputs are absent from the committed WAL. These are
library/storage checks, not implemented account provisioning or HTTP login.

Native release diagnostics on both Rust versions observe one 19,922,944-byte
allocation for correct and wrong password verification, peak equal to that
payload and no live heap afterward. One thousand malformed-record/empty/overlong
rejections request no heap. Cold pool/hash fixtures, allocator overhead/stacks,
caller input and all engine/server memory are excluded; this is not a process
quota, throughput measurement or exhaustive erasure proof.

The parser-only password_records ASan target completes 22,722,625 executions in
46 seconds, RSS312 MiB under a512 MiB guard, input bound256 bytes, without findings.
It never invokes the KDF on fuzz input. Full locked workspace tests pass on stable
1.99.0 and minimum1.89.0:928 main cases,17 ignored process helpers, plus50 optional
release diagnostic cases each with a sequential allocator runner. Formatting,
strict workspace/optional/fuzz Clippy, minimum fuzz compilation, workspace build,
SDK11 unit/7 real native HTTP/restart cases and pinned Python formatting pass.
Both locked graphs pass cargo-audit0.22.2 with warnings denied and no ignored
entries against RustSec b8a1a33e246a0a9a3b5f377248c41a503defec74 (1294 advisories).

Initial test compilation exposed an unsupported generic-slice wipe call and a
diagnostic usize/u64 mismatch; both test issues were fixed before final runs.
No failed run is counted as a pass. [ADR0067](adr/0067-bounded-password-verifiers.md)
and its source-bound artifact retain configuration, ownership, observations and
remaining account/session/worker/security gates. No milestone is closed.

## Private original-engine account store, 2026-10-07

Fifteen new account-library cases cover project binding before I/O, canonical
login names, strict private row shapes/policies, same-name users with distinct
credentials in separate stores, password replacement, disabled/missing checks,
no-op journal equality, positive epoch exhaustion and redacted reports. Missing
identity cannot authenticate even when a synthetic raw fixture's known dummy
digest matches. A256-case pure record property covers complete bounded metadata;
a64-case independent flag/epoch model checks histories, no-op/failure invariants,
compaction, reopen and independently verified restore.

Actual1024-row stores reject growth before hashing;1025 rows fail opening. Seven
semantically invalid user fixtures and eight bad scope/schema cases carry valid
original-engine checksums yet fail account validation. Both WAL versions preserve
old views, committed replacements/epochs, explicit rollback and local backup
restore. Plaintext synthetic password inputs are absent from the retained WAL.
Native private-mode, exclusive-owner, symlink and no-clobber checks execute.

Parser-only account_records ASan completes10,372,743 executions in46 seconds,
RSS289 MiB under512, maximum input256 bytes, without findings. It mutates row
types, lengths, username, epoch and verifier policy; it never invokes a KDF or I/O.
Locked full workspace tests pass on1.99.0 and1.89.0:943 main cases,17 ignored
helpers,50 optional release diagnostics each with a sequential allocator runner.
Formatting, strict workspace/optional/fuzz Clippy, minimum fuzz compilation,
workspace build, SDK11 unit/7 actual native HTTP/restart and both warning-denied
advisory scans pass. Initial compilation caught incorrect engine method names
and a moved property assertion; strict Clippy caught a test range pattern.
Final frozen-source checks pass after those fixes; no failed run is counted.

[ADR0068](adr/0068-private-project-account-store.md) and the
[source-bound artifact](measurements/2026-10-07-private-accounts/verification.json)
keep private-store semantics distinct from future network authority. The current
registry archive does not capture these separately created stores. Coordinated
account/data backup, server-selected authorized paths, account password policy,
worker/crypto admission, HTTP login, sessions/restore revocation and row policies
remain pending. Existing engine kill tests are not new account-specific kill
evidence; physical power-loss and production gates remain open.

## Account-specific forced process termination, 2026-10-07

A dedicated ignored worker is forcibly killed at two controlled boundaries for
each existing WAL version. It parks after a successful account disable/epoch
commit and a flushed acknowledgment, or after staging the same change without
commit. Reopening retains the acknowledged identity/epoch/flag; the staged case
keeps exact original committed WAL bytes. Password behavior, row count and a
verified backup/restore of the recovered account store agree in all four cases.
The parent uses a10-second handshake deadline and cleans up the child before any
assertion, including handshake failure. Test-only worker environment hooks are
absent from production builds. This is process-kill evidence, not physical
power loss or a new account-specific torn-write campaign.

Adding this worker reproduced intermittent Wal(Busy) in a neighboring valid-WAL
fixture during parallel fork/exec. The same condition reappeared on the third
diagnostic repetition. Linux locks follow the
[shared open file description](https://man7.org/linux/man-pages/man2/flock.2.html):
brief inherited descriptors can retain a lock until exec closes them. Account
filesystem test bodies and process launch now share one test-only mutex, following
the existing engine crash-harness discipline. Five repeated default-parallel
account suites pass afterward (16 cases/one ignored helper per suite); KDF-only
thread/admission tests retain their own concurrency checks. No production lock,
permission, corruption expectation or retry path is weakened.

Final full locked workspace runs pass on1.99.0 and1.89.0:944 main cases and18
ignored helpers. Formatting and strict workspace/auth Clippy pass. The prior
50-per-toolchain optional diagnostic runs cover unchanged production behavior;
they are not presented as newly executed in this test-only increment. Earlier
race failures are retained as diagnostic evidence, not counted as successful runs.
The [source-bound kill artifact](measurements/2026-10-07-account-kills/verification.json)
records boundaries, repeated runs and exclusions. Wider media/recovery, integrated
server accounts, coordinated backups, sessions and production gates remain open.

## Purpose/context-bound token primitives, 2026-10-07

A102-byte canonical access/refresh token carries a public family identifier and
an independently generated256-bit secret. Its92-byte EBSK verifier binds purpose,
project, incarnation and family in a fixed SHA-256 preimage. Python hashlib
independently fixes both purpose hashes/layouts; CI reproduces the synthetic
vectors. Fourteen new unit/property cases cover format/matching, complete single
ASCII-byte substitution matrices, header damage, versions, arbitrary Unicode,
opaque record payloads, foreign scope, redacted formatting and owned-secret
zeroization. Tests do not inspect freed memory or claim total-process erasure.

Two new original-engine integration cases cover both WAL versions, unchanged
rollback history, committed replacement, old views, restart and independently
verified restore. Issued plaintext/secret hex is absent from committed journals.
Generic restore still accepts an old verifier under its old incarnation; a
separate expected incarnation rejects it. This is an explicit regression showing
a pending coordinated-restore gate, not an implementation of safe session restore.

ASan parser-only fuzzing completes9,937,783 executions in46 seconds, RSS303 MiB
under512, maximum bounded input512 bytes, without findings. It never issues a
credential, invokes a KDF or accesses storage. Native release samples on both
Rust versions observe zero requested heap bytes across1000 matching/metadata/
decode rounds, including malformed/oversized rejection. Issuance requests one
102-byte output allocation, retains102 while its owner lives and returns to zero
on drop. Caller buffers, stack/RNG internals and total heap remain outside these
requested-payload observations.

Final frozen-source locked runs pass on1.99.0 and1.89.0:960 main cases/18 ignored
helpers and51 optional sequential release diagnostics each. Formatting, strict
workspace/profile/fuzz Clippy, minimum fuzz compilation, workspace build, SDK11
unit/7 native HTTP-restart cases, both independent Python oracles and both
warning-denied advisory scans pass. Initial test compilation caught comparison
of an opaque verifier and a temporary property reference; Clippy caught constant
chunk iteration and a redundant byte conversion. Failed runs are not counted.

[ADR0069](adr/0069-purpose-bound-token-primitives.md) and the
[source-bound artifact](measurements/2026-10-07-session-tokens/verification.json)
retain boundaries: durable sessions, account-state/expiry checks, single-use
refresh, revocation, restore rotation and network admission remain pending.
No stage or production/security gate is completed by cryptographic matching.

## Explicit private session schema, 2026-10-07

Eight new schema/migration cases and one token-context inspection regression
preserve version1 account reading and explicitly upgrade to version2 in one
original WAL commit. No-op migration preserves exact journal bytes. Users,
passwords, old v1 snapshots and account-only counts survive upgrade, compaction,
reopen and independently verified four-schema backup/restore on both WAL versions.
Four forced kills at staged migration and flushed post-commit acknowledgment
recover either complete v1 or complete v2; staged cases preserve exact WAL bytes.
The shared account filesystem/process mutex remains in force.

Actual4096-family storage opens/restores and retains exact history without
inflating account count;4097 families fail opening. Reference validation refuses
missing users, foreign identities, future credential epochs and wrong verifier
purposes/contexts. Older epochs/incarnations remain history, not authority.
Nine bad metadata/schema fixtures fail with valid engine integrity. Complete
row/type/length/grammar/time cases and an independent512-case deadline model cover
clipping and signed limits. A dedicated near-i64::MAX test first reproduces an
incorrect overflow rejection; clipping before addition fixes it. The large fixture
initially exceeded the independent256-event transaction cap and now commits
bounded256-row batches; no engine limit is weakened.

Parser-only session_records ASan completes3,636,274 executions in46 seconds,
RSS426 MiB under512; session_tokens runs6,030,070 in31 seconds,RSS276 MiB.
Both bound input at512 bytes with no findings and no KDF, RNG or storage access.
Final frozen-source locked tests pass on1.99.0/1.89.0:969 main cases/18 ignored
helpers and51 optional sequential release diagnostics each. Formatting, strict
workspace/profile/fuzz Clippy, minimum fuzz compilation, workspace build, SDK11
unit/7 native HTTP-restart, both independent Python oracles and both warning-denied
advisory scans pass. Initial fixture compilation/API mistakes and failed overflow/
capacity runs are not counted as successful verification.

[ADR0070](adr/0070-explicit-private-session-schema.md) and the
[source-bound artifact](measurements/2026-10-07-session-schema/verification.json)
record storage compatibility and open gates. Runtime sign-in/refresh/revoke/prune,
trusted clock/expiry enforcement, account-state admission, coordinated restore
rotation, server integration and production acceptance remain pending.

## Durable session time watermark, 2026-10-07

Ten new account clock cases preserve private versions1/2 and explicitly activate
version3 with one bounded clock row and five exact schemas. Both WAL versions
cover activation from either prior schema, old views, equal-time/no-op history,
forward commits, lower/out-of-range refusal, compaction, reopen and independently
verified backup/restore. Atomic reset changes incarnation/time together while
preserving users; restored old token verifiers fail under the new expected scope.
Deterministic candidates exercise current/retained incarnation collisions and
four-attempt exhaustion without changing the production OS entropy source.

Ten engine-checksummed bad clock/schema fixtures fail opening. Current-incarnation
family issue times above the watermark fail; historical scopes remain data.
Independent64-case advance/refusal/reset sequences preserve exact refusal/no-op
history and verified restored state. Pure shape/bounds properties deliberately
include valid id/version values. Twelve forced kills exercise staged/acknowledged
activation, advance and reset in both WAL versions: recovered scope/time matches,
staged journal bytes stay exact and recovered backups independently restore.
The shared filesystem/process-launch mutex and10-second handshake remain in use.

Pure clock-parser ASan completes21,853,637 executions in46 seconds,RSS329 MiB
under512, bounded input512 bytes, with no findings. Native diagnostics on each
Rust observe zero requested heap bytes/allocations/live bytes for1000 pure clock
inspection rounds, including wrong types, huge bytes and malformed row lengths.
These are requested-payload observations, not whole-process quotas or I/O timing.

Final frozen-source locked tests pass on1.99.0/1.89.0:979 main cases/18 ignored
helpers and52 optional sequential release diagnostics each. Formatting, strict
workspace/profile/fuzz Clippy, minimum fuzz compilation, workspace build, SDK11
unit/7 native HTTP-restart cases, both Python oracles and both warning-denied
advisory scans pass. A missing temporary oracle environment was recreated with
pinned tooling and verified; the interrupted tooling run is not counted as a pass.

[ADR0071](adr/0071-durable-session-time-watermark.md) and the
[source-bound artifact](measurements/2026-10-07-session-clock/verification.json)
keep this metadata protocol distinct from session admission. Future checks must
observe trusted time before every credential attempt. Runtime sign-in/refresh/
logout/expiry, HTTP integration and coordinated restore reset remain pending.
No broad media fault or production/security gate closes here.

## Local durable session lifecycle, 2026-10-07

Eleven new account cases exercise real sign-in, current-state access checks,
refresh, logout, trusted revocation and bounded cleanup. A32-case independent
sequence model tracks account epochs, disable state, current token generation,
revocation and scope reset through compaction, restart and verified backup restore.
A two-thread synchronized refresh test has exactly one winner; the old pair fails
and the replacement admits under current state. Project-bound token records
cannot be copied into another project's store to grant access.

Strict deadline tests cover access/refresh equality, absolute clipping and durable
clock observation before denial. Password replacement, disable/enable and scope
reset invalidate prior credentials. Generation exhaustion, malformed/1MiB token
input, exact no-op revocation and history capacity preserve expected state.
An actual4096-row history reaches capacity,128-row cleanup reclaims it and issuance
then succeeds. Invalid cleanup bounds do not change the clock or journal.

A regression first failed: corrupt current account state was mistaken for an
inactive session during cleanup. After reproducing it, the typed branch was fixed
to propagate corruption/storage errors while accepting only explicit inactivity.
The same fixture now preserves the family and exact equal-time journal bytes.

Eight forced process kills cover staged and acknowledged refresh/logout across
both WAL versions. Commit cases retain generation/revocation and refuse old
credentials; staged cases keep old credentials and exact prior journal bytes.
Independently verified recovered backups reproduce metadata. Child credentials
use a bounded private stdin pipe, excluding command arguments, logs and environment.
This exercises these boundaries, not physical power loss or a new mid-fsync matrix.

Plaintext pairs do not appear in exported WAL. Generic private restore still admits
the latest credential under old metadata; explicit durable reset makes it fail.
Combined registry/account restore must guarantee that reset before traffic.
This is a synchronous private library, without enabled HTTP account/session routes,
roles, row policies or production acceptance. [ADR0072](adr/0072-durable-local-session-lifecycle.md)
records lifecycle and remaining integration gates.

Final frozen-source locked checks pass on1.99.0/1.89.0:990 main cases/18 ignored
helpers and52 optional release diagnostics each. Formatting and warning-denied
workspace/profile/fuzz Clippy, minimum fuzz compilation and workspace build pass.
SDK11 unit/7 real-server restart cases, both independent synthetic cryptographic
oracles and both warning-denied advisory scans pass. No new parser ASan campaign
is claimed for this lifecycle-only change; unchanged parser evidence remains in
the earlier sections. The [source-bound artifact](measurements/2026-10-07-session-lifecycle/verification.json)
records actual counts and the separate unfinished platform gates.

## Private restore preparation, 2026-10-08

Six new private account cases cover versions1/2/3 on both WAL versions, complete
schema/project validation, password/epoch/disable preservation, reset-before-name
publication, immutable source/WAL, old token denial and restart. An independent
32-case disable/epoch model compares restored state. Four forced kills cover
prepared reset and acknowledged publication on both WAL versions. Before exposure
the final directory is absent; after acknowledgement the reset is complete and
old credentials fail, including through an independently recaptured/restored backup.
Two synchronized restorers prepare separate scopes and publish exactly one result.

Ten new backup cases cover trusted preparation and reports describing installed
state, exact ordinary no-op restore, application refusal after commit, malformed
input before callback, changed database identity/corrupted WAL, held application
owner, parent/staging substitutions, no replacement, post-rename selection change
and a64-case independent commit/rollback model. The added sync-failure matrix has
12 combinations across both WAL versions and before/after WAL/directory/parent
sync. Only parent-sync failures occur after exposure and report uncertainty while
preserving transformed state; all source bytes remain exact.

These are actual local library checks. Private remnants after a killed process are
not final directories and may remain inspectable. There is no new physical
power-loss campaign, orphan sweeper, combined platform restore, HTTP route or
production acceptance. [ADR0073](adr/0073-private-restore-reset-before-publication.md)
records the shared publisher and private wrapper boundaries.

Final frozen-source locked checks pass on1.99.0/1.89.0:1006 main cases/18 ignored
helpers and52 optional release diagnostics each. Formatting, warning-denied
workspace/profile/fuzz Clippy, minimum fuzz compilation and workspace build pass.
SDK11 unit/7 real-server restart cases, both independent synthetic cryptographic
oracles and both warning-denied advisory scans pass. No new parser ASan campaign
is claimed for this publication-only change. The
[source-bound artifact](measurements/2026-10-08-private-restore/verification.json)
records counts and distinct unfinished integration/production gates.

## Pure private archive inventory, 2026-10-08

Eight new private account cases share semantic validation across opening, pure
inspection and file/image export. Both WAL versions and private versions1/2/3
preserve exact image/file bytes and source history. Twelve fully engine-valid but
private-invalid fixtures fail all four paths. Actual1024/1025 account and4096/4097
family boundaries execute; revoked history still counts. A64-case independent
inventory model varies account count, version, clock and compaction. Metadata
inspection changes no clock/scope or authority, including through revocation,
disable, reset and partial cleanup.

An owned VerifiedBackup remains readable after input drop, source mutation and
source-directory deletion; its Debug redacts contents. Existing report, ordinary
restore and reset-before-publication behavior use the same checked envelope and
replay. The explicitly invoked ignored corpus helper writes six synthetic archives
covering private versions1/2/3 on both WAL versions. It is not runtime configuration.

The new private_account_archive parser-only ASan target completes85,901 executions
in46 seconds, maximum RSS414 MiB under512, input limit262144 bytes, no findings.
Raw archives reach wire/WAL/private validation; independently modeled short inputs
build checksummed original-engine snapshots with version/scope/verifier/identity/
epoch anomalies. Synthetic opaque digests establish format cases, never password
proof. No KDF or filesystem operations run in the target. Native early scope and
malformed-envelope refusal checks execute1000 rounds without requested heap
bytes/allocations/live bytes; valid archive decoding is allowed to allocate.

[ADR0074](adr/0074-owned-verified-private-archive-inventory.md) keeps inventory
metadata separate from principal authority, whole-process quotas and coordinated
capture. All data/account owners must be retained before a future common capture;
appending an independently later account image does not prove that boundary.
HTTP accounts, combined publication and production gates remain open.

Final frozen-source locked checks pass on1.99.0/1.89.0:1014 main cases/19 ignored
helpers and53 optional release diagnostics each. Formatting and warning-denied
workspace/profile/fuzz Clippy, minimum fuzz compilation and workspace build pass.
SDK11 unit/7 real-server restart cases, both independent synthetic cryptographic
oracles and both warning-denied advisory scans pass. The
[source-bound artifact](measurements/2026-10-08-private-inventory/verification.json)
records parser/native observations and the distinct incomplete integration gates.

## Common registry/private capture, 2026-10-08

Twelve new server cases check the experimental EMILYBND-1 wrapper without enabling
an HTTP account route. Exact registry/private image comparisons preserve source
histories across private versions1/2/3 and both WAL versions. Reversed input roster
produces the same canonical bytes; empty/subset rosters are reported explicitly.
Metadata formatting exposes no tested synthetic login, password or API key.

Boundary callbacks refuse every competing data/private open before the first data
prefix and after a private prefix. Refused outstanding capabilities, external data
owners, duplicate/foreign roster, corrupt private export and changed final registry
metadata release temporary data owners and preserve caller private ownership.
Framing/version/checksum/reserved fields, truncated/tail bytes, overflowing lengths,
unknown/sorted/duplicate scopes and valid nested database identity aliases refuse.
Total encoded-size admission executes exact-limit and overflow checks without
allocating a128 MiB test archive. An independent32-case mutation model checks
registry row counts, explicit roster, private count/time/WAL and source immutability.

Six forced process kills cover all-data-owner, private-prefix and completed capture
boundaries on both WAL versions. Cross-process opens are refused before capture
finishes; exact acknowledged source bytes survive and owners are available after
restart. This is capture evidence, not a new WAL commit/fsync or power-loss campaign.
Two ignored helper tests are invoked only by parent kills or explicit seed generation.

The explicitly invoked corpus helper generates five synthetic bundle seeds:
empty registry and a three-project registry with0..3 supplied private stores.
The account_bundle parser-only ASan target completes350,066 executions in46 seconds,
maximum RSS414 MiB under512, input cap262144 bytes, no findings. Raw inputs and
outer/registry checksum-repaired mutations reach nested validation. Integrity
inspection does not certify a third party's common capture provenance or authority.

[ADR0075](adr/0075-common-registry-private-capture.md) records the common ownership
boundary and explicit roster. Numeric whole-process model/transient/output quotas,
authoritative private catalog, file publication, combined reset-before-root-publication,
HTTP users/roles/RLS and production acceptance remain open.

Final frozen-source locked checks pass on1.99.0/1.89.0:1026 main cases/21 ignored
helpers and53 optional release diagnostics each. Formatting and warning-denied
workspace/profile/fuzz Clippy, minimum fuzz compilation and workspace build pass.
SDK11 unit/7 real-server restart cases, both independent synthetic cryptographic
oracles and both warning-denied advisory scans pass. The
[source-bound artifact](measurements/2026-10-08-account-bundle/verification.json)
records parser observations and distinct unfinished publication/integration gates.

## Private bundle file publication and CLI, 2026-10-08

Twelve new server cases exercise the shared descriptor-owned publisher and bounded
file reader for the account bundle without changing its wire format. They check
exact image/file/report bytes,0600 mode, source history/private owner retention,
no replacement, registry-internal target refusal, malformed input before staging,
final symlinks, FIFO/directory/nonregular files, hard links, broad permissions and
sparse over-cap files. Staged bytes/mode changes and parent/staging/selected-inode
substitutions refuse while preserving foreign/detached objects.

Eight failures injected before/after actual file/parent fsync span WAL1/2. Selected
post-rename states report uncertainty and remain valid; pre-publication failures
leave no final file and permit verified retry. A24-case independent row model
checks output counts, source immutability and refusal to overwrite the first image.
Twelve native process kills span common owner/private capture, file fsync, rename,
parent sync and returned success across both WAL versions. Only complete images
become selected; acknowledged source histories survive and reopen. Two synchronized
native publishers select exactly one complete file and remove the losing owned stage.
Private killed-process remnants are intentionally not adopted or automatically swept.

Three real account-bundle-verify CLI cases check private versions1/2/3 and WAL1/2,
relative Unicode paths, empty/subset inventories and aggregate-only output. Source
and archive histories stay exact; passwords, existing sessions and API keys remain
valid after inspection. Unsafe/corrupt files fail without private stdout/stderr or
new paths.

[ADR0076](adr/0076-owned-account-bundle-file-publication.md) records publication
uncertainty and separate integration gates. No new parser ASan or physical power-loss
campaign is claimed for this file-publication/CLI change. Combined root restoration
with mandatory private scope reset, authoritative roster, HTTP users/roles/RLS,
whole-process resource reservations and production acceptance remain open.

Final frozen-source locked checks pass on1.99.0/1.89.0:1041 main cases/22 ignored
helpers and53 optional release diagnostics each. Formatting and warning-denied
workspace/profile/fuzz Clippy, minimum fuzz compilation and workspace build pass.
SDK11 unit/7 real-server restart cases, both independent synthetic cryptographic
oracles and both warning-denied advisory scans pass. The
[source-bound artifact](measurements/2026-10-08-account-bundle-files/verification.json)
records actual native/file/CLI observations and separate unfinished restore gates.

## Direct original-engine/private byte restoration, 2026-10-08

Three new backup cases verify direct/prepared byte images on both WAL versions,
installed report after private preparation, exact source bytes/history and input
lifetime. No input archive file is created. Malformed images fail before callback
or staging; typed preparation refusal cleans owned staging, and existing foreign
directories/symlinks are preserved. File input keeps its previous private bounded
reader and delegates to the same staging/preparation/publication helper.

The private version/WAL matrix now executes both file and byte entry points:
12 combinations reset scope before publication and preserve restored credentials.
The existing32-case independent disabled/epoch/time model executes the private byte
wrapper. One new private case rejects invalid trusted project/time, wrong scope
and an engine-valid private-invalid epoch without publishing a target. Ordinary
byte restore intentionally retains historical private scope; the private wrapper
is required to deny old tokens before selection.

Native helpers now execute four additional ordinary byte-restore kills at synced/
published boundaries and four additional private byte-restore kills at prepared/
returned boundaries across WAL1/2. Selected states remain complete, restored old
tokens fail, source credentials/history remain exact and separate verified retry
works. No new helper is ignored by default without explicit invocation. These are
process-kill tests, not new physical power-loss or parser ASan claims.

[ADR0077](adr/0077-restore-private-byte-images.md) records shared semantics and the
separate combined root/roster/private reset gate. HTTP accounts, whole-process
resource admission and production acceptance remain open.

Final frozen-source locked checks pass on1.99.0/1.89.0:1045 main cases/22 ignored
helpers and53 optional release diagnostics each. Formatting and warning-denied
workspace/profile/fuzz Clippy, minimum fuzz compilation and workspace build pass.
SDK11 unit/7 real-server restart cases, both independent synthetic cryptographic
oracles and both warning-denied advisory scans pass. The
[source-bound artifact](measurements/2026-10-08-restore-bytes/verification.json)
records actual byte-entry and native recovery observations.

## One-root registry/private restoration, 2026-10-08

Fourteen new server cases exercise offline file/byte restoration under one owned
root, complete inspection and the canonical bounded root manifest. The12 input/
private-version/WAL combinations preserve exact source history and passwords while
resetting every historical private scope before the final root name exists. Old
access/refresh tokens fail and freshly issued sessions work. Generic project API
keys are deliberately preserved. Empty/multiple/subset private rosters remain explicit.

Malformed nested archives and invalid trusted time refuse before staging. Existing
targets, links, source stores and foreign entries are preserved. Root/manifest/
private/history substitutions fail; the entire suspect stage is retained instead
of recursively deleting substituted child entries. A regression test first exposed
that cleanup failure, then passed with retention enabled. Parent/staging/selected
root substitutions distinguish refusal from post-rename uncertainty. Late manifest
replacement, broad permissions and canonical-but-changed reset time are rejected.

The manifest's128-ID boundary and8192-byte cap execute with complete one-byte
corruption, scope/order/version/time, duplicate fields and noncanonical encodings.
Offline inspection rejects extra/missing roster entries, unsafe files and busy
owners. A callback checks every data/private/root owner remains held through final
inventory validation, and all owners are released afterwards.

Sixteen injected failures before/after actual private-directory/manifest/root/parent
fsync cover WAL1/2. Eighteen native forced kills cover registry/private preparation,
manifest, owner retention, final inventory, stage sync, rename, parent sync and ACK.
Selected roots remain complete with old private sessions denied; source histories
and archive bytes remain exact. Independent new-target retry succeeds. Two
synchronized native restorers publish exactly one complete root. A24-case independent
row/disabled/time model checks refusal, retry and restored contents on both WAL modes.

[ADR0078](adr/0078-atomic-account-bundle-root-restore.md) and the
[operator contract](account-root-restore.md) retain separate HTTP account attachment,
authoritative roster, whole-process quotas and production/power-loss gates.

Frozen-source locked checks pass on1.99.0/1.89.0:1059 main cases/22 ignored helpers
and53 optional release diagnostic cases each locally. Workspace/profile/fuzz
warning-denied Clippy, both format checks, minimum fuzz compilation and workspace
build pass. SDK11 unit/7 actual native restart checks, both independent synthetic
cryptographic oracles and both warning-denied advisory scans pass. Final ASan
campaigns execute280755 account-bundle cases and2187494 root-manifest cases in46
seconds each, under512MiB RSS, with no findings. Input caps are262144/8193 bytes;
short campaigns do not prove arbitrary power loss or exhaustive security.
The [source-bound artifact](measurements/2026-10-08-account-root/verification.json)
records these local results and the separate prior hosted schema-allocation failure.
That prior hosted job observed144 process-wide bytes despite serial libtest;100
local repeats pass. A dedicated native measurement follow-up remains required.

## Dedicated schema allocation process, 2026-10-08

Hosted a7e1939 run37703816085 fails the schema-validation diagnostic after observing
144 process-wide bytes;100 local repeats of the original sample pass. A deterministic
background-buffer regression first fails with328 bytes from the unrelated144-byte
buffer and thread startup. The sample therefore moves to an opt-in standalone
native process, keeping strict zero total/block/peak/live assertions for the actual
schema/key operations. No engine, validator, database format or server path changes.

The parent regression now passes while retaining the unrelated background buffer.
A negative control allocates144 bytes inside the child measurement and must report
one block/144 total/peak/live bytes and exit1. The ordinary child reports zero for
1/2/64 columns across1000 iterations each.100 fresh local native processes produce
300 zero samples; negative-control samples are refused. There is no tolerance floor,
counter filtering, warm-up exclusion of validator work or suppressed test failure.

Both1.99.0/1.89.0 pass54 release diagnostic cases and18 default profile-package
cases. Workspace-wide and opt-in all-target warning-denied Clippy and formatting
pass. The preceding869a8cf frozen whole-workspace run passes1059 main cases on both
toolchains; unrelated engine/SDK/parser suites are not claimed as newly rerun for
this isolated diagnostic repair. Hosted results are verified separately after push.

The [source-bound observation](measurements/2026-10-08-schema-allocation-isolation/verification.json) records the failing baseline, strict negative control and native repeats.


## Offline root operator cycle, 2026-10-08

Seven new server and six real CLI cases close restore/verify/re-backup/restore for
an explicit selected root. CLI cases cover all six private-version/WAL shapes,
relative Unicode paths, empty/subset rosters, trusted time boundaries, changed
rows/users/projects, old access/refresh denial and fresh sessions. Output remains
aggregate-only; synthetic names, data, passwords, keys and tokens never appear.
A negative time regression first exposed Clap echoing its input before custom
validation; hyphen-prefixed values now reach the same bounded, redacted validator.

Root capture reuses final inventory validation while every source data/private
owner is held. Exact bytes match explicit common capture; clocks/scopes/histories
remain unchanged. Final source mutation, busy owners, unlisted entries, internal
targets and existing files/links refuse without overwrite. Eight actual before/
after file/parent sync faults, twelve native kills on WAL1/2, two synchronized
publishers with one selected file and sixteen independent row/disabled/WAL cases
execute. Publisher contention starts after separately exclusive source captures;
source Busy is never hidden by retries or weaker locks.

All417 frozen source/dependency hashes remain unchanged through final locked
checks. Both Rust1.99.0/1.89.0 pass1072 main cases/22 ignored helpers and54 optional
release diagnostics each. Formatting, strict workspace/profile/fuzz Clippy,
minimum fuzz compilation and workspace build pass. SDK11 unit/7 real restart
checks, both independent synthetic cryptographic oracles and warning-denied
workspace/fuzz advisory scans pass. This block adds no new ASan campaign or
hardware power-loss claim. Previous869a8cf/6f37e46 hosted runs each complete all
five jobs successfully; the earlier schema-allocation failure is resolved by its
strict dedicated-process diagnostic, including its failing negative control.

[ADR0079](adr/0079-offline-root-operator-cycle.md) and the
[source-bound observation](measurements/2026-10-08-root-operator-cycle/verification.json)
retain HTTP account attachment, automatic roster discovery, whole-process resource
admission and production acceptance as separate gates.


## Retained synchronous account root, 2026-10-08

Thirteen new server cases and a compile-fail lifetime example exercise the exact
inspected owners retained for service use. Opening changes no history/scope/time;
normal restart preserves valid sessions and the persisted time floor. Current
project keys precede private credential/time work. Tokens cannot authorize SQL;
private table names remain unavailable from the public database. Password epochs,
disable state, atomic refresh/logout and key rotation retain their distinct effects.

The four-active-private bound is checked before registry/private opening; empty,
four-store and refused five-store cases execute while offline inspection retains
its wider bound. Missing paths never bootstrap, busy/unsafe owners refuse and all
owners release on failure/drop. Six selected root/private/registry/data/manifest
substitutions refuse operations and preserve foreign/detached objects.

Two actual refresh threads select one replacement pair; it survives reopen.
Four native forced kills cover opened/acknowledged states on WAL1/2. Source history
is exact before service writes, acknowledged families survive and owners release.
Twelve independent generated epoch/disabled/restart/logout sequences check each
historical family's expected authority. Actual service mutations recapture and
restore into an independent root: public rows/users/rotated service keys survive,
old private tokens fail only in the restored clone, and later rows remain isolated.
Unknown/wrong authorized credentials can advance trusted time; equal-time refusal,
backward/overflow no-change and callback suppression are verified separately.

Initial preflight exposed two test calls using access credentials for logout;
logout intentionally requires refresh. Those tests now use the documented
credential and a separate assertion confirms access logout is refused. No lifecycle
semantics were weakened. Final frozen whole-workspace checks are recorded below
only after their actual completion. No new format or ASan campaign is introduced.


Final locked checks keep all420 frozen source/dependency hashes unchanged. Both
Rust1.99.0/1.89.0 pass1086 main cases/22 ignored helpers and54 optional release
diagnostics each. Formatting, strict workspace/profile/fuzz Clippy, minimum fuzz
compilation, workspace build, SDK11 unit/7 actual HTTP restart cases, both synthetic
cryptographic oracles and both warning-denied advisory scans pass. The preceding
f62f68e hosted run37708517914 completes all five jobs successfully. Current hosted
results are verified separately after publication. The
[source-bound artifact](measurements/2026-10-08-retained-account-root/verification.json)
records the actual retained-owner/lifecycle checks and separate unfinished gates.


## Explicit embedded private-root HTTP transport, 2026-10-08

Ten new in-process router/middleware cases use real original-WAL roots. They
provision exact Unicode/NUL passwords, sign in, inspect current principals,
rotate refresh once, deny old pairs, logout with refresh and rotate project keys.
SQL remains service-key scoped; master/cross-project/user-token substitutes refuse,
private schemas stay unavailable and rejected scopes never poll the body.

Unknown/duplicate/client-time JSON, malformed inputs, byte/password limits and
content types refuse without echo or KDF/WAL changes at the same trusted time.
Thirty private attempts exhaust only that project's bucket before KDF; another
project remains available. Pure exact-window/capacity checks enforce128 tracked
project buckets. Four slow bodies consume all permits, the fifth refuses and
unstarted cancellation releases admission. A pending body times out without work.

A started cancelled blocking worker retains its permit/root until an acknowledged
user write completes. Root-lock waiting leaves health/reactor responsive. A trusted
clock rollback fails closed; body time cannot choose a timestamp. Every response,
including refusals and405, carries no-store/no-cache. Static method labels reuse
the existing tested redacted logger; native account log/restart/kill checks remain
part of the next executable-mode increment, not results of these in-process cases.

The first compile rejected a cancellation assertion requiring Debug for a private
error; the assertion now matches cancellation directly. Runtime preflight passes
all ten cases. Dependency graph changes only add the existing zeroize edge to the
server in both lockfiles; locked fuzz checks pass after the explicit offline update.
No engine/manifest/token format or new ASan campaign changes. Final frozen checks
are recorded only after their actual completion.


Final locked checks keep all422 frozen source/dependency hashes unchanged. Both
Rust1.99.0/1.89.0 pass1096 main cases/22 ignored helpers and54 optional release
diagnostics each. Formatting, strict workspace/profile/fuzz Clippy, minimum fuzz
compilation, workspace build, SDK11 unit/7 actual legacy HTTP restart cases, both
synthetic cryptographic oracles and both warning-denied advisory scans pass.
OpenAPI parses, every local schema reference resolves and all five private routes
are described. Preceding6ed8d5b hosted run37710393778 completes all five jobs
successfully. The [source-bound observation](measurements/2026-10-08-private-http/verification.json)
keeps in-process account evidence distinct from pending native account mode.


## Native private-root executable mode, 2026-10-08

Three native executable cases select EMILYBASE_ACCOUNT_ROOT independently of the
legacy registry path. Four invalid configuration combinations fail before creating
paths and never echo private values. One corrupted private WAL1 refuses complete
root startup without overwrite; controlled restoration of the original test bytes
makes it available again without weakening startup validation.

The full real TCP lifecycle executes on WAL1/2. Two simultaneous refresh requests
select one replacement; two SIGKILLs per WAL occur after received refresh/public
write acknowledgements. Restart retains the replacement and rows while denying
old credentials. Current service-key rotation preserves separate user authority.
Offline capture leaves source private history exact; source and verified clone run
simultaneously with distinct session scopes. Clone logout survives graceful restart.
Native logs are screened for master/project keys, sessions, passwords, logins,
identities, row contents and selected paths. These are process kills, not hardware
power-loss evidence. Container root mode and public account policy remain open.

Final locked checks retain all428 frozen source/dependency hashes. Both
Rust1.99.0/1.89.0 pass1099 main cases/22 ignored helpers and54 optional release
diagnostics each. Formatting, strict workspace/profile/fuzz Clippy, minimum fuzz
compilation, workspace build, SDK11 unit/7 legacy real HTTP restart cases, both
independent synthetic cryptographic oracles and both warning-denied advisory scans
pass. OpenAPI parses and all local references resolve. No new ASan campaign or
file/token format change is introduced. See the
[source-bound observation](measurements/2026-10-08-native-account-http/verification.json).


## Explicit first private root, 2026-10-08

Eight new initialization cases cover one empty v3 private project, exact common
captured histories, trusted time/name bounds, protected existing objects, eight
before/after sync faults, six actual process-kill boundaries, eight final path
substitutions, two synchronized native initializers with one winner and eight
independent generated empty-roster models. Corrupt private WAL and foreign-entry
failures execute separately. Failed stages remain private and unselected; successful
publication leaves no stage. Uncertain selected roots remain inspectable.

A new test first failed on late private-container replacement in the initializer
candidate. A separate existing-restore regression reproduced the same gap after
full inspection. Both now run shared retained-owner/inventory checks at the final
selection boundary, alongside the original manifest comparison. The extracted
helper's initial compile errors were corrected before final checks. Both regressions
and all initialization cases pass without weakening filesystem ownership.

Two real CLI cases initialize/verify without identity/name/key/password output,
refuse invalid time/name values before staging and preserve existing paths. One
actual TCP case opens a freshly initialized root, lists/rotates via the operator,
provisions the first user, signs in, commits SQL and preserves authority on restart.
The library lifecycle also restores a copy with independent session authority.

Final locked checks retain all431 frozen source/dependency hashes. Both
Rust1.99.0/1.89.0 pass1111 main cases/22 ignored helpers and54 optional release
diagnostics each. Format, strict workspace/profile/fuzz Clippy, minimum fuzz
compilation, build, SDK11 unit/7 actual legacy HTTP restart cases, both independent
synthetic cryptographic oracles and both warning-denied advisory scans pass.
Prior1af8319 hosted run37712364907 completes all five jobs successfully. Docker is
unavailable locally; this block claims no new container-root or ASan execution.
The [source-bound observation](measurements/2026-10-08-private-root-initialization/verification.json)
keeps hardware power-loss, public policy and production gates open.


## Private HTTP credential management, 2026-10-08

Two new in-process cases exercise current-password change, disable/re-enable,
old access/refresh denial, no-op epoch/history preservation, exact Unicode/NUL
replacement bytes, wrong-current-password no-change and independent other-project
history/authority. Unknown/duplicate/client-time fields, input types, password/body
limits and master/cross-project credentials refuse without history changes at the
fixed trusted time. Shared response helpers verify no-cache headers.

One new actual TCP case runs WAL1/2. For each version, SIGKILL follows received
password-change, disable and re-enable acknowledgements: six kill scenarios per
toolchain. Restart keeps the new password/state/epoch and never revives either
older family. Native logs are screened for credentials. No hardware power-loss,
public reset or role-policy acceptance is claimed.

Final locked checks keep all431 frozen source/dependency hashes unchanged. Both
Rust1.99.0/1.89.0 pass207 server/CLI cases with5 ignored helpers each. This is a
scoped run; the preceding ecc556f complete workspace passed1111/22 and54 optional
diagnostics on each toolchain. Unchanged storage/query/crypto suites are not
reported as newly rerun here. Format, strict workspace/profile/fuzz Clippy, minimum
fuzz compilation, full workspace build, SDK11 unit/7 legacy real HTTP restart cases
and both warning-denied advisory scans pass. OpenAPI has seven private routes,
resolving local references and no duplicate operation IDs. Prior add6c57 hosted
run37713905976 completes all five jobs successfully. Current hosted full-workspace
results are checked separately after publication. No new ASan campaign executes.
See the [source-bound observation](measurements/2026-10-08-http-credential-management/verification.json).


## Private-root container adapter and local native preflight, 2026-10-08

The independent accounts image/Compose target and executable synthetic lifecycle
are recorded in [ADR0085](adr/0085-explicit-private-root-container.md) and
[verification data](measurements/2026-10-08-private-root-container/verification.json).
Native stable/minimum preflights passed against real compiled Rust server/CLI:
one refresh winner, five received-ACK process kills, credential epochs, WAL2
common backup/restore, clone logout, source authority and corruption refusal.
Both probe scripts pass Python format/lint; unchanged Rust/dependency hashes bind
to the preceding checked source. A first probe run rejected Axum's empty405 body;
that helper assumption and a lint-only unused import were corrected before final
checks. No new Rust runtime changes or full workspace/ASan rerun are claimed.

Docker is unavailable locally. The updated hosted container job must execute both
legacy and private targets, including non-root/read-only/cgroup/mode inspection;
its first new private run is pending publication. Native results are explicitly
not container evidence. Stored formats, ACK and all production gates remain.
