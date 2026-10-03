# EmilyBase

An experimental, Apache-2.0 database and self-hosted backend platform written
in Rust. The storage engine is implemented here; no existing database engine
is used as internal storage or a required runtime dependency.

**Early development. Not production-ready. Do not store real user data.**

The current synchronous engine persists named tables, validated schemas and typed
rows in an original versioned page file. It enforces primary-key uniqueness,
nullability and types; supports create/read/update/delete through a working CLI;
and reconstructs tables on reopen. Values include boolean, signed i64, finite
f64, UTF-8 text, bytes and nullable columns. Pages are 4096-byte slotted pages with
CRC32 and strict structure validation. Database files are locked exclusively.

**Synced page writes are not crash-safe transactions.** The current table CLI
uses direct page writes without automatic table repair. A separate WAL crate implements synced commit records,
bounded full-page redo and standalone recovery, with process-kill tests. It is not
yet wired to these table commands. The minimal stage-1 core is implemented; recovery,
concurrent transactions and the backend platform remain future work. Follow
[the roadmap](docs/roadmap.md) for acceptance status.

## Try typed tables

Use synthetic data and a disposable file:

```sh
cargo run -p emilybase-cli -- db-init /tmp/tables.emily
cargo run -p emilybase-cli -- table-create /tmp/tables.emily '{"name":"items","columns":[{"name":"id","data_type":"integer","nullable":false},{"name":"title","data_type":"text","nullable":true}],"primary_key":0}'
cargo run -p emilybase-cli -- row-insert /tmp/tables.emily items '[{"type":"integer","value":7},{"type":"text","value":"synthetic example"}]'
cargo run -p emilybase-cli -- row-get /tmp/tables.emily items '{"type":"integer","value":7}'
cargo run -p emilybase-cli -- row-scan /tmp/tables.emily items --limit 10
cargo run -p emilybase-cli -- row-update /tmp/tables.emily items '{"type":"integer","value":7}' '[{"type":"integer","value":7},{"type":"null"}]'
cargo run -p emilybase-cli -- row-delete /tmp/tables.emily items '{"type":"integer","value":7}'
cargo run -p emilybase-cli -- table-list /tmp/tables.emily
cargo run -p emilybase-cli -- table-drop /tmp/tables.emily items
```

JSON uses explicitly tagged values. Unknown fields and inputs over 16384 bytes
are rejected without printing the input. An absent row prints `null`; updating or
deleting an absent row is an error. Updates retain the primary key. Keys are
integer or text; table/column names are case-sensitive ASCII identifiers.

Current bounds: 128 live tables, 10000 live rows across all tables, 100000 total
history records including the root marker, 64 columns, 3072 bytes per text/byte
value and 4000 encoded bytes per schema/row. Primary-key maps live in memory and
are rebuilt from page events; they are not the planned on-disk B+ tree. There is
no compaction of table history. A last-page cache is bounded to one page.

## Raw page diagnostics

Use a disposable file with synthetic data:

```sh
cargo run -p emilybase-cli -- init /tmp/demo.emily
cargo run -p emilybase-cli -- append /tmp/demo.emily "hello"
cargo run -p emilybase-cli -- get /tmp/demo.emily 1 0
cargo run -p emilybase-cli -- replace /tmp/demo.emily 1 0 "updated"
cargo run -p emilybase-cli -- verify /tmp/demo.emily
cargo run -p emilybase-cli -- delete /tmp/demo.emily 1 0
```

`init` refuses to replace an existing file. Page and slot numbers are physical
addresses; deleted slots may be reused. A record is bounded to 4058 bytes.
The CLI stores UTF-8 text; the storage library supports opaque bytes. The initial
filesystem target is Linux; hard links, directory sync and advisory locks are
required. The CLI accepts trusted local paths. `init` creates a raw-page file;
`db-init` creates a table database. Raw append/replace/delete commands refuse to
mutate table databases. No silent conversion between the formats is performed.

## Development

Install Rust stable, then run:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
```

See [architecture](docs/architecture.md), [file format](docs/file-format.md),
[threat model](docs/threat-model.md), [testing and fuzzing](docs/testing.md), and
[contributing](CONTRIBUTING.md).
