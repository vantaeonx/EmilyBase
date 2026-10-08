# ADR0081: Explicit private-root HTTP transport

Status: accepted for experimental embedded transport; native binary and production gates remain open.

Expose account_router(existing_root, master) over the existing serve transport.
This library increment creates or attaches no project/private store. Native binary
mode selection and its process-level configuration/restart/kill evidence are a
separate next increment; the current binary still uses the legacy registry mode.

Retain one non-cloneable AccountRoot behind an async mutex. Four permits span
memory-only key admission, body reading and blocking work. Filesystem/KDF/WAL
operations run off the reactor; cancellation of a started worker retains its
permit and root lease. Initial serialization and one shared password workspace
are deliberately conservative, not total heap or connection quotas.

All private routes require the current project service key, including provisioning,
sign-in, refresh, logout and account metadata. Never give that key to public
frontend code. User tokens do not authorize SQL. Root mode retains scoped public
SQL/status/explain plus master-gated list/key rotation, without dynamic creation.

Private JSON is bounded to 4096 bytes and five seconds. Unknown fields, including
client time, refuse. Trusted server time comes from the system clock, checked
against the existing persisted floor without clamp/reset/retry. Secret input
owners are zeroized; this does not promise clearing all transport/serde buffers.
Return plaintext session credentials only in an explicit response after durable
success; auth responses, including errors, disable caching. Logs contain static
method/route/status/timing only. Never put session credentials/passwords in URLs,
arguments, environment or logs.

Keep 120 attempts/socket-peer IP/minute and 4096 tracked peers. Add 30 private
attempts/project/minute before body/KDF with at most 128 known project buckets.
Refusals and errors are static redacted JSON. Ambiguous session writes require
inspection/reauthentication; no automatic replay.

Public signup, roles/RLS, dynamic authoritative catalogs, TLS/CORS, encrypted
secrets, whole-process admission and production acceptance remain open. No
milestone closes merely because these experimental routes exist.
