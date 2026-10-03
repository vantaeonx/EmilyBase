# ADR 0013: bounded async HTTP transport over synchronous storage

Status: accepted for the experimental Linux server.

## Decision

Use Axum/Tokio for HTTP and keep storage, recovery and SQL execution synchronous.
Reserve one of four owned semaphore permits before registry/body/engine work;
perform filesystem work only in blocking tasks. The registry mutex supports async
waiting from authorization and blocking acquisition inside workers. Per-project
engine gates and capabilities preserve exclusive directory ownership.

An owned permit stays inside an already started blocking closure through commit
and drop. Request cancellation cannot abort that closure. Body timeouts occur
before writes; no elapsed-time timeout claims to roll back a running commit.
See [Tokio blocking-task lifecycle](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

Separate the configured administrator secret from persisted project-scoped keys.
Expose only implemented JSON routes with strict decoding, bounded bodies, socket-IP
attempt limits and logs containing static route patterns. Default binding is
loopback. Require explicit secret configuration; never ship a default credential.

## Consequences and validation

Four admitted requests limit blocking/body work; they do not cap all connections.
Same-project execution serializes and currently reopens/replays the engine for
each request. This favors straightforward correctness over throughput. Forwarded
headers do not alter rate buckets. There is no transparent retry after uncertain
writes, no user/row authorization, TLS, public deployment or production claim.

A failing regression reproduced reactor blockage while registry publication held
ownership; async waiting fixes it. Actual HTTP tests cover separate scopes,
rotation, independent rows/restart, script rollback, parameters, traversal, body
limits/timeouts, peer attempt bounds and worker saturation. Actual TCP plus
SIGTERM/binary tests verify graceful shutdown, committed reopen and log redaction.
Wider concurrent disconnect/commit crash campaigns and load/security gates remain
open. No page/WAL/index format is changed by this decision.
