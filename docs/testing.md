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
cargo +nightly fuzz run managed_recovery -- -max_total_time=30 -max_len=20000 -rss_limit_mb=512
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
