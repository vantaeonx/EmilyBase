# Experimental private-root HTTP transport

`account_router(existing_account_root, master_key)` builds an Axum router for the
explicit retained root. Pass it to the existing `serve` transport when embedding
EmilyBase. This increment tests in-process middleware/handlers over real original
WAL stores; native binary mode selection and process-level HTTP restart/kill
checks are the next increment. The current executable still uses legacy registry
mode. There is no implicit discovery, private bootstrap or dynamic creation.

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

See [OpenAPI](openapi.json), [ADR0081](adr/0081-private-root-http-mode.md), the
[retained root contract](retained-account-root.md) and [testing](testing.md).
