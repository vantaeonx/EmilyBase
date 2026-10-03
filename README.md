# EmilyBase

An experimental Apache-2.0 database and future self-hosted backend platform in
Rust. The storage engine is original code; no existing database engine is used
as internal storage or as a required runtime dependency.

**Early development. Not production-ready. Use synthetic data only.**

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
The full stage-2 acceptance gate remains open. Persistent table indexes, user authentication, dashboard and Kotlin SDK are future work.
Verified backup/restore works through the library and CLI. Archives contain only
the committed WAL; restore publishes a fully replayed new directory. Process-kill
and competing-publication tests execute. Broader power-loss and upgrade checks
remain open.

The original B+ tree supports unique insertion, leaf/internal splits, pointer
replacement, deletion with sibling rotations/merges and root collapse, sorted
bulk loading, point lookup and ordered ranges. Fixed-size page-image round trips
validate the complete topology. It is a separate bounded library; table primary-key
maps still use the earlier in-memory representation. WAL integration and durable
index files remain future work. Deletion may renumber index page IDs while preserving
external row pointers. See [index format and limits](docs/index-format.md) and
[the integration boundary](docs/adr/0010-index-maintenance.md).
An opt-in stable-ID arena preserves surviving page addresses and reuses holes.
Canonical snapshots and exact-base-bound atomic write sets validate root/counts
and complete topology. A private standalone snapshot publisher and developer CLI
now survive tested creation/replacement kills and competing writers. Atomic
table/WAL integration remains pending.

The original `query` crate implements a bounded SQL lexer, parser, typed AST,
schema-resolved plans and execution through managed WAL transactions: table DDL,
CRUD, predicates, ordering, limits, one inner join and whole-script transaction
control. Separate numbered parameters are supported. CLI SQL errors discard the
entire staged script; commit results follow WAL sync. See [SQL subset](docs/sql.md).
Dedicated SQL process-kill, both-version backup/restore, actual work/output bounds
and independent read/join models execute. The pure read API accepts one SELECT
over a validated snapshot. Wider crash/fault campaigns and durable indexes remain open.

The synchronous project registry creates private isolated database directories,
issues scoped high-entropy API keys and rotates them with atomic metadata publication.
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
