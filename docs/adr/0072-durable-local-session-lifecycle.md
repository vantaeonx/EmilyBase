# ADR0072: durable local session lifecycle on original WAL

Status: accepted for synchronous private library; HTTP and platform gates open.

## Decision

Integrate the existing private account store, EBSK verifiers and version3 durable
clock into real local session operations. No new database engine, async storage,
network route or file/page/WAL/archive version is introduced. Private v1/v2 stores
remain readable and cannot authenticate sessions until explicit clock activation.

sign_in uses the real bounded Argon2 password check, independently selected scope
and current account identity/epoch. It inserts one OS-random family and commits
before returning two zeroizing plaintext owners. At most four family-ID attempts
are allowed. Plaintext credentials are never persisted or formatted through Debug.
Returned metadata is not an authorization credential.

verify_access constructs a borrowed SessionPrincipal privately after full current
state and secret checks. The proof cannot be cloned, implicitly serialized or
constructed externally. Borrowing prevents credential-owner mutation while it
lives. It identifies a user/project/family/generation for one request; roles,
project capabilities and row policies must be enforced separately by future server
integration. A copied metadata value cannot substitute for this check.

## Time, rotation and revocation

Observe trusted integer-second time before all credential checks, even denial.
Equal timestamps do not write; later time is durably committed; earlier time fails
closed after restart. A failed credential attempt can advance this watermark
without changing family state. Trusted time is a service parameter, never an
HTTP-client override. Numeric out-of-range sign-in and invalid cleanup bounds
are refused before staging. Expiry is strict now < deadline.

Access TTL900 seconds, refresh TTL604800 seconds and absolute family TTL2592000
seconds are fixed private-format policies. Refresh clips deadlines to the original
absolute end, increments generation within1..=i64::MAX and replaces both verifiers
in one WAL transaction. Old access and refresh fail immediately. The mutable
single owner and mandatory database lock serialize attempts; one credential has
one durable refresh winner. An ambiguous commit result requires reauthentication,
not automated replay. A client losing the returned replacement cannot recover its
plaintext from stored verifiers.

logout_session verifies the active refresh and commits revoked=true.
revoke_session_family is trusted local administration, with exact no-op behavior
for already revoked state. Current account ID, positive epoch and enabled state
are checked on every admission. Password changes and disable/enable invalidate
old epochs; reset_session_clock atomically changes scope/time and invalidates old
incarnations. Neither current user metadata nor family IDs grant access alone.

## Bounded history and errors

All retained rows count toward4096 family capacity, including inactive history.
Explicit cleanup selects at most128 inactive rows and commits deletion together,
below the engine's256-event transaction cap. Revocation, expired refresh,
changed/missing/disabled account or obsolete scope can establish inactivity.
Corruption and storage errors propagate; they must never be treated as inactivity.
A regression first reproduced cleanup mistakenly accepting any error as inactive,
then verified exact family/WAL preservation after the corrected typed branch.
The clock commit remains separate and can precede a cleanup error.

Bounds limit records/input sizes and transaction events; no total numeric heap,
request-time or platform worker reservation guarantee is claimed. Strict decoders
and typed operations continue to reject malformed input without SQL construction.
No unsafe code or implicit secret serialization is added.

## Recovery and remaining gates

Both original WAL versions preserve acknowledged refresh generation and logout
revocation after a forced process kill. Staged rows are absent after recovery;
previous credentials remain valid and exact previous WAL bytes remain unchanged.
Verified backups of recovered stores reproduce those outcomes. Private child-test
credentials travel through a bounded stdin pipe, never command arguments, logs or
environment variables. Tests do not simulate physical power loss or every fsync
failure. Admission uses the shared account TEST_IO lock around fork-based helpers.

Generic restore deliberately preserves metadata and can therefore accept the
latest token under its old incarnation. Explicit durable reset before traffic
makes that token fail. Coordinated registry/account restoration must perform and
verify reset before publication/traffic; it is not implemented here. Existing
registry backups do not include independently created private stores.

HTTP signup/login, bounded server workers/shared crypto admission, request
throttling/enumeration controls, cookie/CORS policy, combined capture/restore,
roles, row policies and broader security/load/fault gates remain open. The project
is experimental. Actual locked-toolchain, model, concurrent and process-kill
results are recorded in testing.md and the source-bound measurement artifact.
