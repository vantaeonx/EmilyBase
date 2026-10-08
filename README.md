# EmilyBase

An experimental Apache-2.0 database and future self-hosted backend platform in
Rust. The storage engine is original code; no existing database engine is used
as internal storage or as a required runtime dependency.

Opt-in [image replay diagnostics](docs/image-replay-profiles.md) observe physical
plan/output retention and release. Byte reservation and durability gates remain
open before a shared table/index writer can be enabled.
The memory [history append](docs/shared-history-replay.md) now shares unchanged
pages/row bodies and validates canonical new records instead of rebuilding all
prior history. Existing stored formats and acknowledgments remain unchanged.
Optional [parallel replay observations](docs/parallel-replay-profiles.md) also
measure actual scoped workers; numeric runtime memory reservation remains pending.
The standalone [EBIP image envelope](docs/image-plan-format.md) now bounds complete
physical-plan bytes and checks nested scope/count/CRC before owning page vectors.
It requires full exact-state replay afterward and selects no new runtime WAL.
An optional [envelope pool](docs/image-buffer-admission.md) also reserves complete
serialized output bytes before encode/copy and charges explicit clones until drop.
Decoded models, raw plans and whole-process memory remain outside that quota.
Standalone [index validation/deltas](docs/streamed-index-admission.md) now stream
physical checks and avoid redundant complete image sets while preserving frozen
bytes and full topology. Private candidate/retained state still needs admission.
Original [immutable index pages](docs/shared-index-pages.md) now share unchanged
bodies across clones, private candidates and old snapshots. Changed pages detach
without altering historical readers; numeric model/worker admission stays open.
[Primary export](docs/shared-primary-export.md) also admits a shared stable-ID map
without reconstructing a complete image set, retaining full live row/pointer checks.
An optional [decoded plan pool](docs/decoded-plan-admission.md) reserves complete
typed image/address/root vector payloads before owned decode and holds them until
the last shared owner drops. Models, scratch and total heap remain outside that cap.

[Private master-key files](docs/master-key-files.md) now provide bounded startup
configuration in either data mode, with controlled-restart replacement. A separate
Compose variant keeps the master value out of the server environment; the private
file remains plaintext and is excluded from root bundles.

[Logical table exchange](docs/table-transfer.md) now exports/imports one complete
small table through CLI streams, with strict typed validation and a single durable
new-table commit. The explicit255-row limit follows the existing transaction cap.
Project-service HTTP exchange is available in both data modes with a stricter
65,536-byte request/export cap and the same original-engine commit behavior.
[Table schema HTTP](docs/table-schema-api.md) also lists/describes public tables
and creates/drops them through original durable transactions in both modes.
Metadata and transaction IDs are decimal strings; only project service keys apply.
[Typed row HTTP](docs/row-api.md) adds exact-key reads, bounded primary-order pages
and single-row CRUD, with lossless integer strings and finite float bit text.
A bounded row batch applies1..256 ordered writes to one table in one original
commit; any error rolls back the complete packet.
The TypeScript SDK also calls these row operations with exact decimal/bit strings;
its older SQL numeric contract stays separate. Storage and transactions remain Rust.
The Rust [owned staged SQL API](docs/staged-sql.md) also combines typed writes and
SQL before one explicit commit; every error discards the complete transaction.
[Bounded migrations](docs/migrations.md) apply ordered bounded SQL scripts and
receipts in that one commit, with exact historical retries and verified restore.
Original INSERT SELECT also copies bounded typed rows for explicit table rebuilds;
copy overflow or any late conflict discards the complete script.
Project-service migration HTTP in both server modes applies the same atomic contract
and lists exact receipts; concurrent identical requests commit once.
The SDK transports these definitions and preserves full transaction digits,
with explicit retries and conservative handling of lost responses.

A synchronous [row policy decision library](docs/row-policies.md) now binds typed
owner/field rules to current borrowed private principals and complete table identity.
A [bounded record codec](docs/policy-records.md) also preserves complete policy
groups within original page limits. An explicitly enabled [private v4 catalog](docs/policy-catalog.md)
now stores/replaces these groups atomically with actual commit revisions and
current borrowed policy proofs. Verified restore preserves policies and revokes
old sessions. Root-mode [policy administration](docs/policy-administration.md) now
uses current service keys and derives the real table context under a held data owner.
A synchronous [owned user-row gateway](docs/user-row-enforcement.md) now verifies
current sessions and installed policies while holding both original owners through
exact-key reads or an atomic packet of typed writes. Native
[filtered keyset pages](docs/user-row-pages.md) retain only SELECT-permitted rows
and continue by the last visible key. User HTTP routes and roles remain pending.

**Early development. Not production-ready. Use synthetic data only.**

Build/test requirements: Linux and Rust 1.89 or newer. The locked workspace is
tested on Rust 1.89.0 and current stable; CI checks both. Nightly is needed only
for optional sanitizer fuzzing. Docker uses pinned Rust 1.99.0 to build the
original server/CLI. Other operating systems remain unverified.

The synchronous engine supports named tables, validated schemas, primary-key
uniqueness and typed CRUD. Values are booleans, signed i64, finite f64, UTF-8
text, bytes and nullable columns. The original file format uses 4096-byte slotted
pages, CRC32 and strict validation. Managed transactions stage changes, sync a
full-page WAL commit, then publish committed memory state.

Working features include multi-operation commit/rollback, abort-on-write-error,
strict recovery, atomic checkpoint materialization and a CLI. Byte-cut,
process-kill, checkpoint-crash and competing-writer checks execute. The WAL is
capped at 64 MiB. Explicit compaction removes repeated page images into a
self-contained version-2 baseline; checkpoint remains a disposable cache.
The full stage-2 acceptance gate remains open. Persistent table indexes, public user data authorization, dashboard and Kotlin SDK are future work.
The Rust [password helper](docs/password-verifiers.md) supplies tested Argon2id
verifiers and bounded workspaces. The separate [private account library](docs/private-accounts.md)
stores scoped users, password replacements and disable/epoch state on our original WAL engine.
Its explicitly activated session service now implements local sign-in, access
verification, single-use refresh rotation, logout, administrative revocation and
bounded cleanup. Each check uses current account state, project/incarnation scope
and a [durable trusted-time watermark](docs/adr/0071-durable-session-time-watermark.md).
[ADR0072](docs/adr/0072-durable-local-session-lifecycle.md) records deadlines,
commit-before-token-return behavior, concurrency and process-kill evidence.
The [token primitive library](docs/session-token-primitives.md) supplies the
purpose-bound random credentials and strict private verifier formats.
Private versions1/2/3/4 remain readable; session checks require explicit v3 clock
activation, and the policy catalog requires a separate explicit v4 migration.
This library remains separate from server projects and current whole-registry
archives. Explicit private-root transport is described below; public
signup/account policy, roles, user HTTP data admission and automatic
attachment remain pending. The separate
[private restore API](docs/adr/0073-private-restore-reset-before-publication.md)
validates account schema/project and durably resets scope before atomic directory
publication. Use it for private archives; generic engine restore retains old scope.
A [pure private archive inspector](docs/adr/0074-owned-verified-private-archive-inventory.md)
shares complete semantic validation with opening and explicit sensitive exports.
Its metadata report grants no access and does not create/reset a store on disk.
The [offline account bundle](docs/account-bundle-format.md) now captures every
registry data database plus an explicitly supplied private roster while retaining
all source owners across the whole operation. It validates complete nested private
schemas, project scopes and unique database identities. Bundle bytes are sensitive;
automatic private-store discovery remains open.
The [private bundle publisher](docs/adr/0076-owned-account-bundle-file-publication.md)
now saves verified no-replace files; account-bundle-verify inspects them through the
real CLI with aggregate counts only. No private credentials or rows are printed.
The [byte restore APIs](docs/adr/0077-restore-private-byte-images.md) restore nested
images through the same owned publisher without intermediate input archive files.
Use the private account wrapper to reset sessions before publication; ordinary
engine restore preserves historical private scope.
The [retained account root](docs/retained-account-root.md) supplies a synchronous,
current-project-key-gated service over the exact inspected owners. Normal reopening
preserves sessions; fixed-roster admission is capped at four active private stores.
Borrowed user proofs grant no SQL permission. The explicit
[private HTTP transport](docs/private-http.md) now supplies service-key-gated user/
session routes with body/worker/rate bounds. Private password changes and
disable/enable operations durably revoke older credential epochs; re-enabling
does not revive old sessions. Explicit [bounded cleanup](docs/private-http.md#explicit-inactive-session-cleanup)
removes at most128 inactive families per request while preserving refreshable ones;
clock advancement and WAL compaction remain separate. A bounded service-only
[user metadata view](docs/private-http.md#bounded-private-user-metadata-pages)
provides exclusive-login pagination without verifier/session exports or WAL writes.
Set `EMILYBASE_ACCOUNT_ROOT` to an
existing verified root, leaving `EMILYBASE_DATA_DIR` unset, to select this mode
in the actual binary. Real TCP refresh/ACK-kill/restart/clone tests cover WAL1/2;
one damaged declared private store refuses the whole root at startup. See
[executable mode](docs/private-http.md#executable-mode).
The [first-root CLI](docs/private-root-initialization.md) now creates one new empty
project and private session store without an input archive. It prints counts only;
obtain the first usable service key through authenticated operator rotation, then
provision a user through the private HTTP route. Startup still creates no private
store implicitly. A separate [private-root Docker configuration](docs/deployment.md#private-root-container)
uses its own image target and explicit offline initialization; the default registry
container keeps its existing data mode. Native lifecycle preflight and actual
container execution are recorded separately. Late root publication now repeats retained-owner checks for
initialization and restore, including a reproduced private-container substitution.
The [offline root restore](docs/account-root-restore.md) prepares registry data and
every explicitly bundled private store under one owned root. Private session reset
and exact prepared-history validation precede no-replace root publication. API keys
are preserved; automatic HTTP account attachment remains open.
The [operator cycle](docs/account-root-restore.md#operator-cycle) now exposes
account-bundle-restore with required trusted reset time, account-root-verify and
account-root-backup. Root backup captures the exact declared private roster without
resetting source credentials and prints aggregate counts only.
Verified backup/restore works through the library and CLI. Archives contain only
the committed WAL; restore publishes a fully replayed new directory. Process-kill
and competing-publication tests execute. Broader power-loss and upgrade checks
remain open.

Single-database backup/restore now pins destination directory and staged-entry
identities, publishes without replacement through those handles and checks the
selection before success. Parent/staging substitutions are refused; post-rename
errors preserve the selection and report uncertainty. Archive input refuses
symlinks and nonregular files. Native subprocess, sync-fault, generated model and
relative-path CLI checks execute. Linux `/proc` is required; operator-selected
ancestors remain trusted. See [ADR 0032](docs/adr/0032-owned-backup-publication.md).

The independent offline registry publisher now applies the same destination and
staged-inode binding, including restore writes through the original directory
handle. Substitution failures preserve foreign entries and complete ambiguous
selections. Generated scope/epoch models, native mutations, descriptor-release
cycles and real relative CLI cases execute; registry/nested archive bytes are
unchanged. See [ADR 0033](docs/adr/0033-owned-registry-backup-publication.md).

Raw page-file creation now pins its directory and staged inode, rereads exact
synced header/page bytes and publishes through a no-replace rename. Errors after
rename preserve the selected file and report uncertainty. Open refuses link
aliases and nonregular objects. Managed checkpoint operations use the already
owned database directory even if it moves. Native creation kills, sync failures,
generated page models and both-WAL checkpoint regressions execute without changing
file bytes. See [ADR 0034](docs/adr/0034-owned-page-file-publication.md).

Explicit journal compaction now uses the owned database directory throughout
replacement. It checks the selected old WAL inode and the staged/selected new
inode and bytes; source selection changes or uncertain post-rename outcomes
prevent further writes until inspection/reopen. Moving the directory does not
redirect compaction. Before/after-rename substitutions, native mutations and an
independent relocation model exercise this path. See
[ADR 0035](docs/adr/0035-owned-journal-replacement.md).

Managed create/open now anchor relative paths and create/open the mandatory WAL
inside the locked directory through its descriptor. Named directory and selected
WAL identities are checked before return. Raw WAL input rejects final aliases,
multiple links and nonregular objects. A partial managed directory remains
detectable; complete initialization followed by sync/identity failure reports
uncertainty and requires inspection. Creation kills, namespace/sync faults,
independent row models and relative CLI checks exercise this boundary. See
[ADR 0036](docs/adr/0036-owned-database-initialization.md).

The original B+ tree supports unique insertion, leaf/internal splits, pointer
replacement, deletion with sibling rotations/merges and root collapse, sorted
bulk loading, point lookup and ordered ranges. Fixed-size page-image round trips
validate the complete topology. Managed snapshots now derive point-lookup trees
from live row locations; SQL primary-key equalities use this original routing.
Long text keys retain the existing map path. Durable table/index WAL participation
remains future work. Deletion may renumber index page IDs while preserving
external row pointers. See [index format and limits](docs/index-format.md) and
[the integration boundary](docs/adr/0010-index-maintenance.md).
An opt-in stable-ID arena preserves surviving page addresses and reuses holes.
Canonical snapshots and exact-base-bound atomic write sets validate root/counts
and complete topology. A private standalone snapshot publisher and developer CLI
now survive tested creation/replacement kills and competing writers. Atomic
table/WAL integration remains pending.

Borrowed double-ended B+ cursors now read intervals in either direction without
copying a whole result vector. Reverse traversal uses bounded ancestor paths,
so existing EBIX-1 bytes are unchanged. `index-range PATH --descending --limit N`
inspects an ordered standalone key/pointer interval. See
[ADR 0027](docs/adr/0027-double-ended-index-cursors.md).

`Snapshot::primary_rows` now borrows live rows in either direction, validating
consumed short-tree entries and every selected physical image. Long keys merge
before the caller's limit; long bounds use checked point lookups. Full imported
tree verification also streams keys instead of allocating a complete key vector.
See [ADR 0028](docs/adr/0028-borrowed-live-primary-rows.md).

Single-table SELECT now uses borrowed rows when no order is requested or the first
ORDER BY field is the unique primary column. It reads in that direction, evaluates
the full filter and stops after LIMIT matches, retaining projected fields only.
Ordinary non-primary orders now borrow checked sources and keep only the best
LIMIT full candidates. General fallback joins keep materialized limits. Explain uses the
existing `primary_range`; `sorted` indicates requested ordering. Wide limited reads,
script budgets, both-WAL restore and real CLI/HTTP kill replay are tested. See
[ADR 0029](docs/adr/0029-streamed-primary-sql-order.md).

UPDATE/DELETE now select checked borrowed candidates within the transaction's
remaining event capacity. UPDATE copies selected rows only; DELETE retains keys
only. Prior statements share the 256-event bound, and overflow rolls back the
whole script. Zero matches consume no events. A reproduced memory-capped selection
abort, generated models, CLI/HTTP and both-WAL recovery checks cover the change.
Snapshot staging now shares unchanged tables and copies a changed table on first write. See
[ADR 0030](docs/adr/0030-bounded-mutation-selection.md).

Derived trees preserve all 10000 admitted rows: a full dense build uses 768 pages,
and fragmented incremental arena exhaustion triggers a rebuild. Initialized
trees stage insert/delete/pointer maintenance alongside their relational snapshot.
`primary-index-info PATH TABLE` reports the derived tree and excluded long keys.
It does not create an index file. See [ADR 0021](docs/adr/0021-derived-primary-key-trees.md).

Integer primary-key inequalities in necessary AND conjuncts now use linked-leaf
range lookup for SELECT/UPDATE/DELETE, with checked i64 boundaries and unchanged
null/rollback semantics. Explain reports `primary_range`. See
[ADR 0022](docs/adr/0022-integer-primary-range-plans.md).

Text primary ranges also support necessary AND inequalities in UTF-8 byte order.
Short bounds validate linked-leaf entries; live ordered keys include excluded long
keys before LIMIT. SQL uses this path for representable bounds through 256 bytes;
longer SQL bounds retain full intervals with filtering. No locale collation is
claimed. See [ADR 0026](docs/adr/0026-utf8-primary-range-plans.md).

Explicit bound primary-tree images now validate persistent database/table identity,
acknowledged transaction, exact relational history and every eligible live pointer
before installing a derived cache. Checkpoint, compaction and verified restore
preserve matching images. Explicit private sidecar publication/load is available;
recovery now attempts them after mandatory WAL replay within a 16-MiB input budget,
reconstructing rejected/missing/skipped caches from live rows. See [image format](docs/table-index-image-format.md) and
[ADR 0023](docs/adr/0023-bound-primary-tree-images.md).

`primary-index-save PATH TABLE` atomically writes a private optional cache;
`primary-index-load PATH TABLE` explicitly verifies and loads it (null if absent).
Publication holds database ownership and uses descriptor-relative operations,
file/directory sync and no-clobber creation. Damaged/stale images cannot install;
cache failures leave confirmed table data usable. Both commands print only counts
and identifiers. Backups omit disposable sidecars. See [ADR 0024](docs/adr/0024-private-primary-cache-files.md).

`primary-index-cache-status PATH` reports startup counts without exposing keys.
All 128 tables and 10000 rows fit the tested cache budget. File growth during read,
stale/foreign images, failed WAL and an ACK-before-cache-refresh kill are covered.
Confirmed table durability never depends on saving a cache. See [ADR 0025](docs/adr/0025-bounded-primary-cache-startup.md).

Validated live row-image locations now bind table/key to an actual slotted-page
position and event fingerprint. Managed locations also bind the persistent
database identity. Updates/deletes retire previous images; discarded staged
changes publish none. Reopen, baseline compaction and verified restore retain
current locations. See [ADR 0020](docs/adr/0020-validated-live-row-locations.md).

The original `query` crate implements a bounded SQL lexer, parser, typed AST,
schema-resolved plans and execution through managed WAL transactions: table DDL,
CRUD, predicates, ordering, limits, one inner join and whole-script transaction
control. Separate numbered parameters are supported. CLI SQL errors discard the
entire staged script; commit results follow WAL sync. See [SQL subset](docs/sql.md).
Dedicated SQL process-kill, both-version backup/restore, actual work/output bounds
and independent read/join models execute. The pure read API accepts one SELECT
over a validated snapshot. Wider crash/fault campaigns and durable indexes remain open.

The experimental `commit-format` crate implements fixed-size typed page addresses
and root metadata for the planned shared table/index commit. Database-global
history pages and per-table primary pages have separate namespaces; roots bind
exact predecessor revisions, transactions and fingerprints. This standalone codec
does not enable a new WAL version or write indexes in managed transactions.
See [experimental metadata](docs/commit-metadata-format.md) and
[ADR 0037](docs/adr/0037-experimental-commit-namespaces.md).

The separate `commit-model` prototype stages complete relational/index changes,
validates every selected tree against current row images and publishes one immutable
memory state after exact-base checks. Old views and refused plans remain unchanged.
Full 10000-row model cases cover integers and text keys at both the 256-byte
tree boundary and 3072-byte relational boundary. Dense integer/short-text index
rebuilds select 768 pages. See [capacity evidence](docs/durable-index-capacity.md).
The model bounds combined candidate/selected index images to 2048 pages, exposes
checked encoded component sizes and reuses immutable validated index hashes.
Full 128-table capacity accepts 1536 fragmented pages. These are image/component
bounds, with heap/server reservation still open. See
[ADR 0039](docs/adr/0039-combined-model-index-images.md).

An opt-in release diagnostic now measures requested allocations for synthetic
model stages and retained views. Four held 10000-row long-key models peak near
940 MiB in one local shape; this is a measured clone/retention cost, not a memory
quota or parallel-worker bound. Full and streamed fingerprints match, with lower
allocation traffic on the streamed path. The diagnostic allocator is absent from
server/CLI runtime dependencies. See [reproduction and actual reports](docs/model-allocation-profiles.md)
and [ADR 0040](docs/adr/0040-opt-in-model-allocation-diagnostics.md).
The model has no file/WAL writes or durable acknowledgments; combined budgets and a shared
durable writer remain pending. See [ADR 0038](docs/adr/0038-staged-table-index-model.md).

Relational snapshot clones now share immutable tables and per-table physical
location maps. First mutation detaches only the affected table; unrelated rows
and historical views keep their objects. No-op/read-only/index-only stages avoid
eagerly copying all rows. A write still copies the affected shared map structure, so
heap/server admission remains open. Keys and unchanged row bodies now remain
shared inside a detached table/map as well; only map structure and changed rows
are newly owned. The synthetic four-model row-write peak falls to about 554 MiB.
Existing durable formats/ACKs are unchanged. See
[ADR 0042](docs/adr/0042-shared-row-bodies-and-live-keys.md) and
[ADR 0041](docs/adr/0041-shared-relational-snapshot-tables.md).

An optional `ModelPool` now admits registered projects, distinct retained/pending
memory generations, reader objects and writers before staging. Private leases
release on discard/failure or after the last old reader; publication checks exact
owner identity. This count-based prototype writes no files and is not a heap-byte
or server quota. See [model lifetimes](docs/model-lifetimes.md) and
[ADR 0043](docs/adr/0043-bounded-model-lifetimes.md).

Raw prepared memory states can materialize changed physical history/index images
and independently replay both projections against their exact base. Plans enforce
separate image bounds, preserve committed slots and reject partial/foreign roots.
The real 10000-row/768-image case keeps the 256-history-image bound separate.
This adds no wire format or durable acknowledgment. See
[physical plans](docs/model-image-plans.md) and
[ADR 0044](docs/adr/0044-validated-physical-image-plans.md).

The synchronous project registry creates private isolated database directories,
issues scoped high-entropy API keys and rotates them with atomic metadata publication.
Live capabilities pin registry/project/data directory identities; replacement
paths fail before scoped execution instead of redirecting requests.
Only key digests are stored. Cross-project keys, traversal IDs and unsafe symlinks/
permissions are rejected; requests for each project serialize. The Axum server
adds separate administrator/project scopes, bounded blocking workers, strict JSON,
peer attempt limits, structured logs with static HTTP-method labels and graceful shutdown. Actual TCP and binary
checks execute, including both-version writer kills, concurrent projects, accepted
request drain and damaged-journal isolation. Accounts/roles remain future work. See [HTTP server](docs/server.md),
[OpenAPI](docs/openapi.json) and [project registry](docs/projects.md).

Offline whole-registry backup/verify/restore preserves every current project's
committed history, identities and rotated access digests in a bounded private
archive. Restore verifies complete replay before no-clobber publication; master
keys stay external. See [registry backup format](docs/registry-backup-format.md).
Broader crash/fault and production gates remain open.

```sh
cargo run -p emilybase-cli -- sql /tmp/emilybase-demo 'SELECT id,title FROM items WHERE id=$1 LIMIT 10' --parameters '[{"type":"integer","value":7}]'
cargo run -p emilybase-cli -- sql /tmp/emilybase-demo 'SELECT * FROM items WHERE id=7' --explain
```

## Try typed tables

For the compiled Rust server/CLI in Docker, see [local container deployment](docs/deployment.md).
Compose uses a private named volume and loopback port. Actual recreation, SIGKILL,
corruption isolation and offline backup/restore checks run; this remains experimental.

Use a disposable managed directory and synthetic data:

```sh
cargo run -p emilybase-cli -- db-init /tmp/emilybase-demo --durable
cargo run -p emilybase-cli -- table-create /tmp/emilybase-demo '{"name":"items","columns":[{"name":"id","data_type":"integer","nullable":false},{"name":"title","data_type":"text","nullable":true}],"primary_key":0}'
cargo run -p emilybase-cli -- row-insert /tmp/emilybase-demo items '[{"type":"integer","value":7},{"type":"text","value":"synthetic example"}]'
cargo run -p emilybase-cli -- row-get /tmp/emilybase-demo items '{"type":"integer","value":7}'
cargo run -p emilybase-cli -- row-scan /tmp/emilybase-demo items --limit 10
cargo run -p emilybase-cli -- row-update /tmp/emilybase-demo items '{"type":"integer","value":7}' '[{"type":"integer","value":7},{"type":"null"}]'
cargo run -p emilybase-cli -- row-delete /tmp/emilybase-demo items '{"type":"integer","value":7}'
cargo run -p emilybase-cli -- table-list /tmp/emilybase-demo
cargo run -p emilybase-cli -- checkpoint /tmp/emilybase-demo
```

Table/row commands recognize the managed directory and commit through WAL.
JSON uses tagged values. Unknown fields and inputs over 16384 bytes are rejected
without echoing input. An absent row prints `null`; updating/deleting it is an
error. Updates preserve the primary key. Names are case-sensitive ASCII identifiers;
primary keys are integer or text.

For managed databases, `row-location PATH TABLE KEY_JSON` prints a bound location
or `null`. `row-resolve PATH TABLE KEY_JSON LOCATION_JSON` checks its identity and
current row image before printing the row. These development commands require
ordinary local data access; locations are not credentials or transaction IDs.

Current limits: 128 tables, 10000 live rows in total, 100000 history records,
64 columns, 3072 bytes per text/blob and 4000 encoded bytes per schema/row.
Primary-key maps are in memory, rebuilt from history. Managed mode retains bounded
page images and copies live row state at transaction start. This is a correctness
baseline; shared concurrent readers and high throughput remain future work.

## Multi-operation transactions

`tx` accepts a JSON array up to 16384 bytes and 256 operations. Supported `op`
values are `create_table`, `drop_table`, `insert`, `update`, `delete`. A failed
write aborts the whole batch. Using the `items` table created above:

```sh
batch='[{"op":"insert","table":"items","row":[{"type":"integer","value":10},{"type":"text","value":"synthetic batch"}]}]'
cargo run -p emilybase-cli -- tx /tmp/emilybase-demo "$batch" --rollback
cargo run -p emilybase-cli -- tx /tmp/emilybase-demo "$batch"
```

An I/O failure during commit has an unknown outcome. Reopen and inspect its
transaction ID before retrying; absence of a response does not prove rollback.
Recovery requires `redo.wal`. Missing/corrupt WAL fails closed even if an older
checkpoint exists. Checkpointing does not reduce WAL size and is not a verified
backup. Interrupted initialization is rejected; an existing path is preserved.

## Explicit journal compaction

```sh
cargo run -p emilybase-cli -- compact /tmp/emilybase-demo
```

This retains all relational events, schemas, rows, table IDs and transaction
numbers while removing redundant page images. New transactions continue at the
previous ID plus one. The replacement is synced, recovered and compared before
atomic rename and directory sync. Directory ownership spans WAL inode replacement.
Opening never changes format; only this explicit operation writes WAL version 2.
Old version-1 readers reject it. Backups support both versions. History and the
64 MiB limit remain bounded; compaction cannot reclaim obsolete table events.
Process-kill tests cover staging, rename, directory sync and returned success for
both source versions. Sync failures poison the owner until reopen. Actual 64 MiB
capacity tests verify refusal without mutation, compaction and new durable writes.
Physical power loss, broader filesystem failures and history vacuuming remain open.

## Verified backup and restore

Create an archive, verify it independently and restore into a new directory:

```sh
cargo run -p emilybase-cli -- backup /tmp/emilybase-demo /tmp/emilybase-demo.backup
cargo run -p emilybase-cli -- backup-verify /tmp/emilybase-demo.backup
cargo run -p emilybase-cli -- restore /tmp/emilybase-demo.backup /tmp/emilybase-restored
cargo run -p emilybase-cli -- row-scan /tmp/emilybase-restored items --limit 10
```

Existing files, directories and symlinks are preserved. Backup requires exclusive
source ownership. Verification checks sizes, CRCs, SHA-256, identity, commit
boundaries and relational history. It prints metadata without row contents.
Restore preserves database identity and accepts new commits. Backups contain
plaintext data and stay outside Git. Linux local filesystems are the current
target; encryption, incremental copies and format upgrades remain future work.

## Legacy files and raw diagnostics

`db-init PATH` without `--durable` creates the legacy table file. Existing files
remain readable without silent conversion. **Direct page writes in this mode are
not crash-safe transactions.** Raw commands use a separate disposable file:

```sh
cargo run -p emilybase-cli -- init /tmp/emilybase-raw.emily
cargo run -p emilybase-cli -- append /tmp/emilybase-raw.emily "synthetic example"
cargo run -p emilybase-cli -- get /tmp/emilybase-raw.emily 1 0
cargo run -p emilybase-cli -- replace /tmp/emilybase-raw.emily 1 0 "updated"
cargo run -p emilybase-cli -- verify /tmp/emilybase-raw.emily
cargo run -p emilybase-cli -- delete /tmp/emilybase-raw.emily 1 0
```

Raw append/replace/delete refuse to mutate initialized table files. Slot/page
numbers are physical addresses; deleted slots may be reused. Raw records are
bounded to 4058 bytes. The CLI accepts trusted local paths. The filesystem target
is Linux with local hard links, directory sync and advisory exclusive locks.

## Development and privacy

```sh
cargo fmt --all -- --check
cargo fmt --manifest-path fuzz/Cargo.toml -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
cargo clippy --locked --manifest-path fuzz/Cargo.toml --bins -- -D warnings
```

Open source covers the code license. Rows, journals and checkpoints stay local
and are excluded from Git; publishing this repository does not publish database
contents. Never commit real data, passwords, tokens, signing keys or `.env` files.

See [architecture](docs/architecture.md), [page format](docs/file-format.md),
[journal format](docs/wal-format.md), [threat model](docs/threat-model.md),
[backup format](docs/backup-format.md),
[index format](docs/index-format.md),
[recovery matrix](docs/recovery-matrix.md), [testing](docs/testing.md),
[roadmap](docs/roadmap.md), [size estimate](docs/size-estimate.md) and
[contributing](CONTRIBUTING.md).

The [TypeScript SDK](sdks/typescript/README.md) executes scoped SQL/explain/status
with runtime validation, bounded response reads and explicit unknown write outcomes.
Node unit and actual server/restart checks run in CI. It is a local experimental
package, not an npm release; browser/CORS and Kotlin work remain open.


## Follow-up: admitted destination replay lifetimes

[Destination replay](docs/admitted-model-replay.md) now reserves per-project/global
writer exclusion and its output generation before complete physical reconstruction.
Borrowed AdmittedPlan input keeps separate vector accounting. Non-clonable output
retains leases through exact-instance publication/discard and exposes only borrowed
state. Actual ownership, refusal, historical readers, generated sequences and
thread caps execute under [ADR 0054](docs/adr/0054-admitted-physical-replay-generations.md).
This counts lifetimes, not model/cache/staging/transient heap or runtime workers;
the combined durable writer and production gates remain open.


## Follow-up: compact retained relational payload

[Retained record ownership](docs/retained-record-capacity.md) now removes caller
spare String/Vec capacity after validation, before shared live state retains it.
Reproduced schema/row/native regressions, exact wire parity, old readers, generated
histories and WAL1/2 commit/rollback/recovery checks execute under
[ADR 0055](docs/adr/0055-compact-retained-record-payloads.md).
Caller input peaks and model/cache/map/staging/replay-transient budgets remain
outside this retained-shape guarantee; durable-index and production gates stay open.


## Follow-up: compact retained index buffers

[Retained index ownership](docs/retained-index-capacity.md) now removes caller
spare text/key/pointer/child-vector capacity before immutable page publication.
Reproduced constructor/insertion/native failures, both ID policies, old owners,
full capacity, format parity and standalone reopen checks execute under
[ADR 0056](docs/adr/0056-compact-retained-index-buffers.md).
Input peaks and allocator/model/cache/staging/replay-transient budgets remain
outside this shape guarantee; the combined durable writer and production gates
remain open.


## Follow-up: primary-key inner join probes

[Eligible JOIN plans](docs/primary-key-joins.md) now probe the original right
primary index from a borrowed left scan. Complete ON/WHERE, long-key physical
checks, ordering/limits and existing work/output bounds stay active. Necessary
equality under AND is supported; other conditions keep the original fallback.
EXPLAIN and the matching strict project SDK accept primary_join under
[ADR 0057](docs/adr/0057-primary-key-join-probes.md). This enables no new stored
format, durable-index acknowledgement, chained/outer join or production gate.


## Follow-up: stream ordered primary joins

[Unique left-primary JOIN order](docs/ordered-primary-joins.md) now uses the
original point/range/double-ended source cursor and stops after accepted LIMIT
matches. Streamed plans retain selected output fields while complete ON/WHERE
and shared work/output budgets stay active. Other orders retain full stable sort
and intermediate limits under [ADR 0058](docs/adr/0058-streamed-primary-join-order.md).
Stored formats, durability acknowledgements and production gates are unchanged.


## Follow-up: bounded primary-join sorting

[Small sorted JOIN limits](docs/limited-primary-join-sort.md) now retain only
the best LIMIT full candidates with stable tie/null/type ordering. Full ON/WHERE
and every source probe still execute; work, matched-row, retained-byte and output
bounds stay active under [ADR 0059](docs/adr/0059-bounded-primary-join-sort.md).
Other access paths keep their original limits. Stored formats, durable-index
acknowledgement and production gates are unchanged.


## Follow-up: admit projected payload before copying

[Output admission](docs/projected-output-admission.md) now checks the shared
script allowance before copying each selected row. Full sorting projects
incrementally; streamed paths use the same borrowed preflight. A reproduced
native refusal peak falls from 99388593 to 14210993 bytes under
[ADR0060](docs/adr/0060-admit-projected-output-before-copy.md), with identical
errors/committed bytes. This is logical output accounting; whole-memory, durable
index and production gates remain open.


## Follow-up: borrowed ordinary table sorting

[Non-primary table orders](docs/limited-table-sort.md) now borrow checked
source rows and retain only the best LIMIT candidates. Necessary points/ranges
restrict access, while complete WHERE and every candidate still execute. Stable
ties, all supported sort types, original work/match/retained/output bounds and
WAL 1/2 recovery hold under [ADR 0061](docs/adr/0061-borrowed-bounded-table-sort.md).
The reproduced warmed 1500-row peak falls from 4842026 to 11634 requested bytes.
This operation-local observation closes no cold-memory, transient, combined
durable-writer or production gate. General fallback joins retain their limits.


## Follow-up: borrow before candidate admission

[Borrowed candidates](docs/borrowed-query-candidates.md) let primary JOIN
predicates/projection read checked source slices directly. Sorted table/JOIN heaps
compare and charge winners before cloning; discarded candidates still consume
the original match/work counts. Reproduced warmed allocation totals nearly halve
under [ADR 0062](docs/adr/0062-admit-borrowed-candidates-before-cloning.md).
Physical validation still allocates; these operation totals are not a process
quota. Stored formats, durability acknowledgements and production gates remain.


## Follow-up: borrowed physical row comparison

[Physical verification](docs/physical-row-comparison.md) shares one validating
cell codec with original owned decode. Checked row resolution compares borrowed
payload with immutable live state, retaining identity/digest/full-format checks
under [ADR0063](docs/adr/0063-compare-physical-rows-with-borrowed-codec.md).
Warmed1000-resolution requests fall from3344000/6416000 to104000 bytes; schema
validation still allocates. This closes no cold-memory, combined durable writer
or production gate and changes no stored format or acknowledgement.


## Follow-up: bounded stack schema validation

[Schema checks](docs/stack-schema-validation.md) retain every original rule
and error order using bounded borrowed-name arrays. Oversized names do not enter
sort comparisons. Native schema/key and physical-resolution samples request no
temporary heap under [ADR0064](docs/adr/0064-bounded-stack-schema-validation.md).
These observations exclude stacks/cold fixtures and close no numeric whole-memory,
combined durable writer or production gate. Stored bytes and ACK are unchanged.


## Follow-up: borrowed complete schema inventory

[Snapshot metadata](docs/borrowed-schema-inventory.md) now supports borrowed,
exact-size iteration in live table-ID order. Model preparation and complete root
validation preserve every check while avoiding column copies; status reads a
count, and cache warmup retains only bounded table names. The 128-by-64 warmed
prepare sample falls from1,459,120 to22,192 requested bytes, peak723,160 to4,784.
A separate1000-pass inventory scan allocates zero. [ADR0065](docs/adr/0065-borrowed-schema-inventory.md)
records ownership, ordering, evidence and limits. File/WAL/SQL/HTTP meanings stay
unchanged. Numeric model/cache/staging/transient budgets and the combined durable
writer remain open; this does not complete a production gate.


## Follow-up: borrowed nested-loop sources

[Fallback joins](docs/borrowed-nested-join.md) now retain bounded checked row
references and evaluate complete ON/WHERE before any candidate copy. Full matched
row limits, stable sort/source ties, pair/predicate work and projection timing
remain unchanged. With100 rows per side and3000-byte hidden payload, false ON/
WHERE summed requests fall64,469,242/64,468,894→9530/9182. [ADR0066](docs/adr/0066-borrowed-nested-loop-sources.md)
records evidence and limits. The planner retains bounded_nested_loop; this adds
no fallback TopK or public point/range extraction. Numeric model/cache/staging/
transient budgets, combined durable writer and production gates remain open.
