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
process-kill, checkpoint-crash and competing-writer checks execute. The full WAL
is retained and capped at 64 MiB; checkpoint reuse and log rotation are pending.
The full stage-2 acceptance gate remains open. SQL, B+ tree, server, project
isolation, authentication, dashboard and SDKs are future work.
The backup library now creates and verifies committed-WAL archives and restores
them into a fully replayed new directory; its CLI and interruption tests follow.

## Try typed tables

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
```

Open source covers the code license. Rows, journals and checkpoints stay local
and are excluded from Git; publishing this repository does not publish database
contents. Never commit real data, passwords, tokens, signing keys or `.env` files.

See [architecture](docs/architecture.md), [page format](docs/file-format.md),
[journal format](docs/wal-format.md), [threat model](docs/threat-model.md),
[backup format](docs/backup-format.md),
[recovery matrix](docs/recovery-matrix.md), [testing](docs/testing.md),
[roadmap](docs/roadmap.md), [size estimate](docs/size-estimate.md) and
[contributing](CONTRIBUTING.md).
