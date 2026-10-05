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
