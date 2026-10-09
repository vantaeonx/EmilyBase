# ADR0106: Trusted backend user-row HTTP with two separate credentials

Status: accepted for the two-credential trusted backend adapter after the checks below; no user-only public authority.

Expose get/page/write only in retained account-root mode under /auth/rows. Require
the current project service Bearer key plus exactly one X-EmilyBase-Access header
with the existing 102-byte access token. Never distribute a service key to clients.
No legacy route, public signup, user-only row/SQL/DDL authority or role grant is
added. Existing service SQL/row routes keep their trusted authority.

Reuse the original strict row point/page/batch grammar and lossless integer/float
wire values rather than introducing a second decoder. Map only get/page/batch to
the synchronous owned policy gateway. Body/response caps remain 65,536 bytes;
private peer/project attempt limits, four shared workers, five-second body waits
and no-cache responses apply. Copy the access header into a zeroizing owned value;
this does not promise erasure of HTTP internals. Logs contain route/status, never
credentials, definitions or row bodies.

After body completion recheck current service key/private roster before decoding,
then derive the real held table context and verify current session/policy/time
inside the original root. Keep both owners through decision and commit. Key,
policy or session changes during a body wait apply to the operation. Exact-key
reads mask absence/SELECT denial; pages use only permitted rows/visible keys.
Writes stage one complete packet and acknowledge the original durable commit.
Private forward clock observation remains separately durable.

Return static 400 input/typed-key rejection, 401 credential rejection, 403 policy/
packet rejection, 409 disabled catalog and 503 corruption/binding/clock/storage or
response uncertainty. Validate schema-specific input keys before physical lookup,
so client type errors do not become ambiguous storage responses. Persisted physical
failures remain storage errors. Lost results require inspection; no automatic
retry/idempotency receipt is added. An oversized filtered page fails as a whole.

Acceptance requires strict/duplicate/header/body/size/full-u64 cases, owned packet
rollback and hidden reads/pages, current key/policy/session changes during waits,
private rate bounds, legacy absence, unchanged sibling/equal-clock private history,
real TCP received-result and unread-response kills on WAL1/2, explicit duplicate
retry refusal, verified common-root clone with fresh sessions, existing HTTP/
native/owned-core regression, stable/minimum checks, OpenAPI and sanitizer fuzzing.
Public client admission, roles, SDK adapters, security/load/upgrade/resource and
production gates remain open.

Frozen-source checks pass 70 selected cases on each Rust toolchain: 39 private HTTP,
two transport, 17 owned-core and 12 real account-network cases. Ten regular cases
are new. The new native scenario receives four write ACKs before forced kills and
leaves two write responses unread before independent inspection/kill across WAL1/2,
per toolchain. Verified clones preserve nonempty rows/policies and require fresh
sessions; source sessions remain usable. Original core staged/ACK packet tests also
rerun. An actual typed-key request first reproduced 503 instead of 400, then passed
with explicit pre-lookup key validation. Strict formatting/Clippy, minimum workspace/
fuzz builds, OpenAPI and 3,288,259 ASan request/mapping runs pass without findings.
See [source-bound evidence](../measurements/2026-10-09-trusted-user-row-http/verification.json).
