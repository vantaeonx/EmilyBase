# EmilyBase

An experimental, Apache-2.0 database and self-hosted backend platform written
in Rust. The storage engine is implemented here; no existing database engine
is used as internal storage or a required runtime dependency.

**Early development. Not production-ready. Do not store real user data.**

The first increment establishes the Rust workspace and design documents.
Follow [the roadmap](docs/roadmap.md) for implementation and acceptance status.

## Development

Install Rust stable, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

See [architecture](docs/architecture.md), [file format](docs/file-format.md),
[threat model](docs/threat-model.md), and [contributing](CONTRIBUTING.md).
