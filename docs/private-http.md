# Experimental private-root HTTP transport

`account_router(existing_account_root, master_key)` builds an Axum router for the
explicit retained root. Embed it with the existing `serve` transport, or select
the executable mode below. In-process middleware and actual TCP process
restart/kill cases use real original-engine WAL1/2 stores. There is no implicit
discovery, private bootstrap or dynamic creation.

## Executable mode

Create the first empty root with [account-root-init](private-root-initialization.md),
or restore/verify an independent root through the
[offline operator cycle](account-root-restore.md#operator-cycle). Select an existing
verified root explicitly:

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
Do not add a second root variable to that configuration. The separate [private-root Compose adapter](deployment.md#private-root-container)
uses an independent accounts image target and explicit offline initialization;
its original lifecycle passed in the hosted container job.

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
| `/password` | `login`, `current_password`, `replacement_password` | 200, updated account metadata |
| `/disabled` | `login`, `disabled` boolean | 200, updated account metadata |
| `/sessions/prune` | `limit` integer1..128 | 200, `removed` count |
| `/users/list` | `limit` integer1..128; optional `after` login/null | 200, metadata page |

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


## Private credential management

Password change requires the current password and the current project service key.
A successful durable change increments the credential epoch and invalidates all
previous access/refresh families. Replacement bytes remain exact UTF-8, including
Unicode/NUL, within1..1024 bytes; both passwords share the4096-byte encoded JSON
bound. Even selecting the same password advances the epoch under the existing
store contract. Passwords belong in the private JSON body only.

The trusted disabled-state route requires the current service key. A changed state
increments the epoch; re-enabling cannot revive older families. Repeating the same
state preserves epoch/WAL. These operations expose existing synchronous account
semantics with shared worker/rate/body/no-cache/error rules, not user roles or public
password reset. Unknown/duplicate/client-time fields refuse. No automatic cleanup,
automatic replay, public signup or SQL user authority is added. See
[ADR0084](adr/0084-private-http-credential-management.md).


## Explicit inactive-session cleanup

POST /v1/projects/{id}/auth/sessions/prune requires the current private project
service key and a strict JSON object containing limit, an integer from1 to128.
No client time or extra fields are accepted. The response is only {"removed":N},
where0<=N<=limit, after the bounded deletion commits. Revoked, refresh-expired and
stale account/incarnation families are eligible. Access expiration alone preserves
a refreshable family. Other projects and public SQL histories remain unchanged.

The server observes its trusted clock separately before scanning. A backward clock
refuses; zero removals can still persist a forward watermark. At the same time,
an empty repeated cleanup preserves exact WAL bytes. Row deletion releases family
capacity but appends WAL history: offline compaction remains a separate operation.
No automatic scheduling, session identifiers, remaining-count receipt or byte-space
reclamation is supplied. Invalid limits refuse before private time advancement.
The existing worker/body/rate/no-cache/static error and uncertain-outcome rules
apply. See [ADR0086](adr/0086-bounded-private-session-cleanup.md).


## Bounded private user metadata pages

POST /v1/projects/{id}/auth/users/list requires the current project service key.
Strict JSON contains limit from1 to128, plus optional after as a canonical ASCII
login or null. The exclusive boundary need not name an existing user. No query
string, client time, unknown/duplicate field or implicit normalized cursor is used.
Response users contain only the existing ID/login/credential_epoch/disabled shape;
next_after contains the last emitted login only when the current read sees another
record, otherwise null. No password, verifier, session, scope or remaining count
is returned. This trusted service view grants no user SQL authority.

Each page reads current state. Continuation retains no cross-request snapshot:
insertions behind a previous cursor may be absent, and newer entries ahead may
appear. Consumed private records, including lookahead, are completely validated;
corruption refuses instead of returning a partial page. Bounds precede page
allocation. Listing never hashes a password, observes session time or commits WAL.
The worker/rate/body/no-cache/static error rules apply, including current key
revalidation after a waiting body. See [ADR0087](adr/0087-bounded-private-user-pages.md).


The fixed-root router also exposes project-service
[table export/import](table-transfer.md#project-http-transport). It operates only
on the authorized public database, rechecks the current service key after body
admission, and does not observe or advance private session time/WAL. Master or
user session tokens cannot grant this authority. Its body/export cap is65,536
bytes; the separate4096-byte private credential-body cap stays unchanged.
