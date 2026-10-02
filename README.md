# EmilyBase

An experimental, Apache-2.0 database and self-hosted backend platform written
in Rust. The storage engine is implemented here; no existing database engine
is used as internal storage or a required runtime dependency.

**Early development. Not production-ready. Do not store real user data.**

The first increment includes an original synchronous page engine and a working
CLI. It creates a versioned database file, stores compactable records in 4096-byte
slotted pages, validates CRC32 and page structure, and locks files exclusively.
It supports physical record create/read/update/delete and detects corrupt or
truncated files. Tables and transactions are not implemented.

**Synced page writes are not crash-safe transactions.** There is no WAL or
automatic repair. This is an initial part of stage 1, not a completed database
or backend platform. Follow [the roadmap](docs/roadmap.md) for acceptance status.

## Try the CLI

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
required. The CLI accepts trusted local paths.

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
