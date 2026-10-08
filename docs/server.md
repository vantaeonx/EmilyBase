# Experimental HTTP server

Linux/local filesystem, disposable synthetic data only. This is an implemented
Axum transport over the original synchronous engine, not a production backend.
No existing database service is required. Start from the repository root:

```sh
export EMILYBASE_MASTER_KEY="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
export EMILYBASE_DATA_DIR=/tmp/emilybase-http-demo
cargo run --locked -p emilybase-server
```

The master secret is exactly 64 lowercase hexadecimal characters (256 random
bits). Supply it outside Git; never put it in URLs or public frontend code.
Alternatively supply [EMILYBASE_MASTER_KEY_FILE](master-key-files.md) and remove
EMILYBASE_MASTER_KEY entirely. Exactly one source is required; file keys accept one
optional final LF and exact private0400/0600 mode. Missing/invalid/conflicting
sources fail before data-directory creation. The default listen
address is `127.0.0.1:7000`; override with `EMILYBASE_LISTEN`. Master-key replacement
currently requires a controlled restart. No remote hosting or paid service is
configured. The private root directory must have mode 0700, metadata files 0600.

## Implemented routes

The JSON contract is [OpenAPI 3.1](openapi.json). Send the exact
`Authorization: Bearer KEY` header. Project and administrator scopes are separate:

| Method/path | Key | Behavior |
| --- | --- | --- |
| GET `/health` | none | static `{"status":"experimental"}`, not storage readiness |
| GET `/v1/projects` | master | public project metadata, no digests or keys |
| POST `/v1/projects` | master | `{ "name": "demo" }`, 201 with project and new key |
| POST `/v1/projects/{id}/keys/rotate` | master | new key, increasing epoch; no body required |
| GET `/v1/projects/{id}/status` | project | last committed transaction, table/row counts |
| POST `/v1/projects/{id}/tables/export` | project | complete bounded typed table document |
| POST `/v1/projects/{id}/tables/import` | project | create one new table from a typed document |
| POST `/v1/projects/{id}/sql` | project | atomic SQL script and typed parameters |
| POST `/v1/projects/{id}/explain` | project | resolve one SELECT plan without writes |

Create/rotate return the raw new key once. Only its digest is persisted. Save the
returned key privately. Project keys currently grant all operations within their
own project; there are no user accounts, granular roles or row policies yet.
Rotation denies old keys on new authorization; an already accepted request can
finish with its captured capability. Master keys do not grant project SQL access.

Create a project with a JSON request, then use its returned `project.id` and
`api_key` with a SQL payload such as:

```json
{
  "sql": "CREATE TABLE items(id INTEGER PRIMARY KEY, title TEXT); INSERT INTO items VALUES (1,$1); SELECT * FROM items",
  "parameters": [{"type":"text","value":"synthetic example"}]
}
```

Each submitted script is one transaction. A script error discards every staged
write; successful changed scripts respond after WAL sync. SELECT-only scripts
preserve the committed transaction ID. Initialization itself commits transaction
1; the first later changed script uses 2. Explicit rollback reports
`committed:false` and may return staged reads. See [SQL semantics](sql.md).

## Bounds and lifecycle

At most four admitted requests hold permits across authorization, body reads,
blocking execution and completion. Saturation returns 503 `workers_busy` without
starting work. Registry waits use an async mutex; disk/recovery/query operations
run in blocking workers. Per-project gates serialize database operations.
Cancelling a request before starting disk work releases admission; once a blocking
commit begins, it continues and retains its permit/ownership even if the client
disconnects. No transaction timeout forcibly cancels an in-progress commit.

Bodies require `application/json`, are at most 65536 bytes and must finish in five
seconds before execution. Unknown JSON fields and malformed tagged values fail
without echo. SQL has its own 16384-byte parser limit, 256 parameters, depth/work/
row/output budgets. The 8 MiB query output budget estimates retained rows; it is
not a serialized JSON limit, and escaping can make wire responses larger. Responses
are buffered, not streamed. JSON i64 values require clients that preserve signed
64-bit integers; JavaScript must reject integers outside its exact safe range.

Protected routes allow 120 attempts per socket-peer IP per monotonic 60-second
window. Failed authentication consumes the same budget. Peer tracking is capped
at 4096; expired entries are reclaimed. Forwarded headers are ignored. Behind a
reverse proxy, clients share the proxy's bucket; trusted forwarding configuration
is not implemented. Health/unknown routes bypass this limiter. Four workers bound
engine admission, not the total number of HTTP connections; broader network/load
limits remain an acceptance gate.

SIGINT/SIGTERM stop accepting new connections and drain accepted requests. Logs
contain method, static route pattern, status and elapsed time. They exclude paths
with project IDs, query strings, headers, bodies, SQL, tokens and returned rows.
Method labels are static: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, CONNECT,
TRACE, or OTHER. Extension methods, including token-shaped or lowercase names,
are never copied into logs. This applies to authentication failures and 405s.

## Failure responses

Errors use `{ "code": "..." }` without credentials, SQL or filesystem paths.
401 is generic `access_denied`; 400 is `invalid_json`, `invalid_project_request`
or `query_rejected`; 408 `body_timeout`; 413 `body_limit`; 415 `json_required`;
429 `rate_limit`; 503 covers `workers_busy`, storage and uncertain outcomes.
Infrastructure/extractor/panic faults can return 500. Unsupported routes/methods
use Axum's 404/405 responses rather than this JSON contract.

An uncertain commit/publication or disconnected response must be inspected after
reopen using transaction state. Never automatically retry a write: absent response
does not prove rollback. No request-id deduplication is implemented.

TLS termination, CORS, user auth, row policies, database-wide export/import,
realtime, objects, dashboard and Kotlin SDK are future increments. The project
TypeScript client is implemented; see its [usage and bounds](../sdks/typescript/README.md).
The Rust server can also run through [Docker/Compose](deployment.md). Stop it before
using [offline whole-registry backup/restore](registry-backup-format.md); there is
no HTTP backup route. Wider backup/publication crash/fault campaigns and production
security/load gates remain open.

Registry kill/sync-failure tests and actual network writer kills now execute.
Received SQL responses survive both WAL versions; complete unobserved commits can
also survive. Missing/corrupt WAL fails closed per project while healthy siblings
remain available. Cache repair still requires an explicit checkpoint.


## Explicit private-root executable mode

Set EMILYBASE_ACCOUNT_ROOT to an existing verified offline-restored root and leave
EMILYBASE_DATA_DIR unset. The [private transport contract](private-http.md) describes
service-key-gated accounts/sessions, whole-root startup validation, fixed roster
and native WAL1/2 recovery evidence. Normal restart preserves sessions; restore
resets clone authority. This mode creates no project or private store implicitly.


[Logical table HTTP exchange](table-transfer.md#project-http-transport) uses the
existing65,536-byte/five-second body admission and a65,536-byte export cap in both
data modes. It disables caches, checks service scope and returns import metadata
only after durable commit. It grants no user-token SQL or row-policy authority.
