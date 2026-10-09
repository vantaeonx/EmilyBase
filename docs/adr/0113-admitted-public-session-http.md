# ADR0113: separate admitted public session HTTP

Status: accepted for explicit admitted public sessions after these checks.

## Context

The native current-user gateway and offline operator admission are verified.
Existing HTTP auth routes require project service credentials and cannot be used
by an ordinary client holding only its own session. An explicit separate network
boundary is needed before exposing owned rows or adding a user SDK/dashboard.

## Decision

In account-root mode only, add four POST routes under /v1/projects/{id}/user:
sign-in, refresh, logout and me. Use a separate UserScope and middleware with no
service/admin capability. Sign-in accepts the existing Login object; refresh and
logout accept the existing Refresh object and reject Authorization headers. Me
requires exactly one Authorization: Bearer access token of102 ASCII bytes and an
exact empty JSON object. A project service key or master key is never a fallback.
No signup, user listing/modification, SQL, DDL or row route is added here.

Share the original four worker permits, actual socket-peer limiter and30 attempts
per private project per minute with existing private routes. Ignore forwarded IP
headers. After the peer/header/permit checks, perform memory-only current flag
preadmission under the retained root. Unknown, missing, legacy or closed projects
deny before body work and cannot fill the bounded project-rate map. The precheck
does not authorize subsequent work or perform filesystem I/O/KDF/time observation.

Use the original4096-byte/five-second JSON body reader and wiping secret owners.
Require JSON objects before typed Serde parsing, preserving strict duplicate and
unknown-field rejection. This also corrects existing private account routes that
accepted undocumented positional arrays through Serde struct deserialization.
Expose a pure bounded SessionRequest grammar validator for tests/fuzzing; it grants
no credential or project authority and performs no KDF/storage/clock operation.

After all body/root waits, acquire the real root in a bounded blocking task.
Check its current flag before consulting the trusted server clock, then invoke the
original native public method, which checks selected filesystem identities,
current flag and original session/password state. The same exclusive owner stays
held throughout. No caller clock or detached flag/session proof is trusted.
Use original current-user metadata/session encodings with exact decimal strings,
original static failure mapping, no-store headers and static matched-route logs.
No password/token/request body/project ID/raw path enters the log.

Closing suspends all four routes without revoking families. Intentional reopening
can resume a current unexpired session. Service-key rotation is independent of a
current admitted user session. Verified clone closes admission and changes session
incarnation; reopening the copy still requires fresh login. Existing service-key
routes retain their authority and refuse user credentials.

## Reproduced corrections

A new strict-empty-body test observed an array reaching native authentication,
and a test on existing private sign-in confirmed a positional login/password array
issued a session. Both were reproduced before adding object-only shared decoding.
Another test closed admission after body preadmission with a deliberately failing
server clock: it returned503 instead of401 because the clock ran before the native
flag check. The blocking adapter now rechecks the flag before reading the clock;
the test also asserts zero clock calls and unchanged WALs.

## Verification plan and boundaries

Stable/minimum Rust: closed/legacy/unknown refusal before body/clock/map work;
current purpose/project/epoch/refresh/logout and service independence; header/JSON
bounds, duplicates and secret-safe errors; delayed body/root current state; shared
workers, cancellation, timeout/health, known-project budget and actual-peer limit;
concurrent refresh. Actual TCP login/refresh/logout responses must survive three
forced server stops per WAL, and nonempty verified copy must remain closed until
explicit reopening and fresh login. Rerun original private HTTP, native public
gateway and account network suites; validate all OpenAPI references/security.
Strict lint/format/minimum compilation and bounded ASan request fuzzing are required.

No public row route, CORS/cookie authentication, signup, role, user SDK/dashboard or
production gate is enabled. Plain local HTTP testing does not prove external TLS,
power-loss durability, whole-runtime resource bounds or an independent security
audit. Lost session mutation results remain inspection-required, without automatic
retry. Closing admission is suspension, not revocation.


## Final verification

Rust1.99 and1.89 pass79 checks each: private/public account HTTP49, native public
gateway9, actual account network13, original HTTP7 and server documentation1.
Ten regular cases are new, nine router cases and one actual TCP matrix. Six new
forced stops per toolchain occur after received login/refresh/logout HTTP responses
across both WALs. Existing native24 generated models/four packet kills are rerun.
Nonempty verified copy remains closed and old tokens stay invalid after opening.

The final pure request ASan campaign executes10418523 inputs in46 seconds,
RSS366MiB under512, max input8192 bytes/request4096,56 initial seeds, no findings.
This does not fuzz the whole HTTP/header/credential flow. Strict workspace/fuzz
formatting/linting, minimum workspace build and all-target fuzz compilation pass
on500 frozen source/dependency/API hashes. OpenAPI has40 operations and442 resolved
local references. Added1221 Rust lines: total106436 source, Rust102149
(98271 effective), SDK2502 and Python1785. See
[source-bound evidence](../measurements/2026-10-09-public-session-http/verification.json).
Public user rows, signup/roles, client/browser integration, load/upgrade/resources
and independent security/production gates remain open.
