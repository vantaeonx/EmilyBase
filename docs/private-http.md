# Experimental private-root HTTP transport

`account_router(existing_account_root, master_key)` builds an Axum router for the
explicit retained root. Embed it with the existing `serve` transport, or select
the executable mode below. In-process middleware and actual TCP process
restart/kill cases use real original-engine WAL1/2 stores. There is no implicit
discovery, private bootstrap or dynamic creation.

## Executable mode

After the [offline operator cycle](account-root-restore.md#operator-cycle) creates
and verifies an independent root, select that existing root explicitly:

```sh
env -u EMILYBASE_DATA_DIR EMILYBASE_ACCOUNT_ROOT=restored-root \
  cargo run --locked -p emilybase-server
```

Supply the required private `EMILYBASE_MASTER_KEY` through operator configuration
as for legacy mode. `EMILYBASE_LISTEN` defaults to loopback port7000. Explicitly
setting both data-directory variables refuses before filesystem work; so do an
invalid master/listen configuration. Account mode opens existing paths only and
never bootstraps, resets sessions or changes the fixed roster. Omit both variables
to retain the legacy `emilybase-data` default. Legacy dynamic project creation
remains separate from account-root mode.

Startup validation/opening runs in a blocking worker for both modes. The owned
master input is zeroized after router construction; process environment/transport
copies are outside that erasure guarantee. One corrupt declared private WAL
refuses the whole root before listening; healthy sibling availability is not a
root-mode promise. A normal restart retains session authority; verified restore
creates an independent private scope and denies old tokens in the clone.

The existing Docker image and default Compose configure the legacy data variable.
Do not add a second root variable to that configuration. A dedicated verified
container root-mode adapter remains a separate deployment increment.

## Private routes

**Service keys must stay on a trusted backend.** Every private route requires the
current project service key in `Authorization: Bearer KEY`. It is not safe to
place that key in a browser/mobile public client. This is private control-plane
provisioning and session transport, not anonymous public signup or user SQL access.
Master keys do not substitute for project keys. User access/refresh credentials
never authorize project SQL. Roles and row policies remain unimplemented.

| POST path under `/v1/projects/{id}/auth` | Strict JSON body | Success |
| --- | --- | --- |
| `/users` | `login`, `password` | 201, account metadata |
| `/sign-in` | `login`, `password` | 200, new access/refresh pair |
| `/refresh` | `refresh_token` | 200, replacement pair; old pair denied |
| `/logout` | `refresh_token` | 200, `logged_out:true` |
| `/me` | `access_token` | 200, current account metadata |

Account metadata contains a32-hex-character user ID, canonical login, decimal
string `credential_epoch` and `disabled`. Session responses explicitly contain
`access_token`, `refresh_token`, `token_type:"Bearer"` and decimal string
`expires_at` for the access deadline. No password, digest, family history or
incarnation is returned. Token owners are serialized only after durable success.
An interrupted/uncertain refresh needs inspection/reauthentication; never blindly
retry it. No deduplication or automatic refresh retries are supplied.

Root mode also provides the existing service-key `/sql`, `/explain`, `/status`
paths and master-gated project listing/key rotation. POST `/v1/projects` is405:
the selected roster is fixed. Unlike a detached legacy SQL capability, root
operations recheck the current service key at blocking execution after the body;
rotation can therefore reject a previously admitted but unstarted root operation.
Explicit generic projects without private stores keep SQL; their private routes
refuse without provisioning directories.

Private bodies require application/json, finish within five seconds and contain
at most4096 encoded bytes. Unknown/duplicate fields, including client time, fail
without echo. Passwords are exact UTF-8 bytes, including NUL, bounded to1..1024
bytes by the existing password library. Inputs are never normalized or truncated.
Raw parsing copies and secret field owners are zeroized on drop; transport and
Serde can retain other copies, so this is not a whole-heap erasure claim.

Four worker permits cover authorization, body reads, blocking work and completion.
Filesystem, password and WAL operations run in blocking tasks. An async mutex
serializes initial root operations; waiting never blocks the reactor. A started
worker keeps its root lease/permit after client cancellation and completes its
write. Unstarted cancellation releases admission. This does not bound total
connections, response lifetimes or combined model/cache/transient heap.

Keep the existing120 attempts/socket-peer IP per monotonic60-second window with
4096 tracked peers. Additionally allow30 private attempts per known project/window,
before body/KDF; project tracking is capped at128. Forwarded headers are ignored.
These counters are process-local and reset on restart; they are not durable bans.
The root admits at most four private stores before opening databases.

Service time comes from the system Unix clock, checked against the persisted
watermark. No body time, archive time, clamp or automatic scope reset substitutes
for it. A backward clock produces503 `trusted_clock_unavailable`; an authorized
forward denied credential attempt can advance the clock. Normal reopening retains
valid sessions; explicit restore resets them before directory selection.

All root responses, including errors/405s, send Cache-Control:no-store and
Pragma:no-cache. Request logs contain only static method/route/status/timing,
never bodies, headers, path IDs, query strings or credentials. Errors remain static
JSON:401 access_denied;400 invalid_json/invalid_account_request;409 account_exists/
account_capacity;408 body_timeout;413 body_limit;415 json_required;429 rate_limit/
account_rate_limit;503 workers_busy/password_workers_busy/trusted_clock_unavailable/
session_outcome_requires_inspection/storage_unavailable. No precise storage outcome
can be inferred from a disconnected response. TLS/CORS, public account policy,
roles/RLS, dynamic catalogs and production/security acceptance remain open.

See [OpenAPI](openapi.json), [ADR0081](adr/0081-private-root-http-mode.md),
[ADR0082](adr/0082-native-private-root-mode.md), the
[retained root contract](retained-account-root.md) and [testing](testing.md).
