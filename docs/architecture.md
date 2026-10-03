# Technical design

EmilyBase will provide project-isolated relational storage and a backend API.
The engine is original Rust code. PostgreSQL wire or SQL compatibility is not
promised. Initial operating-system target: Linux with a local filesystem.

```mermaid
flowchart TD
  CLI[CLI] --> Storage[Storage: synchronous pages and files]
  CLI --> Database[Database: table coordination and event replay]
  Database --> Catalog[Catalog: schemas and typed records]
  Database --> Storage
  HTTP[Future Axum server] --> Query[Future parser / planner / executor]
  Query --> Transactions[Future transaction coordinator]
  Transactions --> WAL[Future WAL]
  Transactions --> Database
  Catalog --> Index[Future B+ tree]
  Index --> Storage
  WAL --> Storage
  HTTP --> Auth[Future authentication and policies]
  HTTP --> Realtime[Future committed-change subscriptions]
  HTTP --> Objects[Future private object storage]
  Backup[Future backup service] --> Transactions
```

Storage, catalog, database and CLI crates are implemented. Add other crates when they contain
working behavior, instead of declaring an implemented platform with empty modules.
The future network layer will call the synchronous engine through bounded workers;
blocking filesystem work must not run on Tokio reactor threads.

## Storage boundary

File page 0 is an immutable format header. Data pages start at 1. Each page
stores bounded opaque records. The relational layer stores bounded typed events
in those records and reconstructs tables and primary-key maps on open. Its
initialized marker distinguishes it from raw page files. Page and slot addresses
are physical, not public row IDs. See [typed record format](catalog-format.md).
One open pager owns an exclusive advisory file lock. Writes require mutable
access. Checksums detect accidental corruption; they do not authenticate data.

## Transaction boundary (planned)

Start with a single serialized writer and strict locking. A commit will append
bounded redo records and a commit marker to WAL, sync WAL, and only then
acknowledge success. No page becomes durable before its corresponding WAL.
Recovery replays complete committed transactions; checkpoint completion must be
durable before WAL truncation. MVCC is deferred until the baseline is proven.

## Platform boundary (planned)

Each project receives a server-controlled directory and catalog. Public IDs must
never be concatenated into filesystem paths. Authorization must bind every
operation, subscription and object access to a project. The dashboard uses
React/TypeScript/Vite; REST uses Axum, Serde and OpenAPI; realtime uses WebSocket.
TypeScript and Kotlin SDKs will use the documented API. Docker and Compose follow
once a runnable server exists. No external paid service is required.
