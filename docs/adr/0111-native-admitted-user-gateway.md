# ADR0111: native admitted user gateway without service credentials

Status: accepted for the native current-user boundary after these checks.

## Context

Private v5 now has an explicit closed admission flag. The existing policy-enforced
row path still requires a trusted project service key alongside a user token.
A network adapter must eventually operate without placing service credentials in
browser/mobile applications. First implement and verify its synchronous authority
boundary while retaining the original storage, session and policy protocols.

## Decision

Add native AccountRoot public_sign_in, public_refresh_session,
public_logout_session, public_user and public_user_table. Every operation selects
the actual retained private store, checks current selected filesystem identities
and reads its current v5 flag. Missing project/private store/catalog or a closed
flag denies before password validation/KDF, time observation or public data work.
No detached receipt authorizes anything. Trusted time is supplied by the service,
never an assertion accepted from an eventual HTTP request body.

Reuse the original bounded password verifier, current private session state,
single-use refresh and logout. public_user returns only the current account's own
metadata after verifying its access token. A service key or refresh token does not
act as an access credential. No signup, user listing, administrative user change,
SQL, schema manipulation or unfiltered storage handle is offered to users.

For rows, keep the original typed get/page/write shapes and limits. Authenticate
access before opening public data or resolving table metadata. Obtain the current
policy proof under both actual owners and consume it through the existing original
row executor. The mutable private owner remains held through public ownership,
context checks and the full original commit. Native Root ownership prevents flag,
credential or policy mutation while that borrow lives. Current row ownership,
USING/CHECK, hidden-row/page semantics, primary uniqueness, complete rollback on
late failure and migration-ledger exclusion remain required.

Factor the existing held-data policy execution body so trusted service+user calls
retain their contract. Factor project capability construction without changing
service-key authorization. A new crate-private owned-project selector exists only
for this retained-root path; it never becomes an external capability factory or
an HTTP service-key fallback. Public methods consume that internal capability
locally and return only bounded rows, commit metadata or current user metadata.

## Lifecycle and recovery

Closing admission suspends all five public methods, including login/refresh/logout,
without changing credential epochs or destroying families. An explicit later reopen
can resume an unexpired current session. Disabling/re-enabling an account, changing
its password, expiry, logout or refresh rotation still invalidate the corresponding
old credentials. Service-key rotation is independent: current public user sessions
continue, while the old key cannot perform trusted service administration.

Verified private/common-root restore closes admission and replaces session
incarnation/time before publication. Explicit operator reopening of the copy cannot
revive source tokens; fresh login is required. Data, identities and policies survive.
The source can remain active with its own original current session and unchanged WALs.
Generic engine restore still requires explicit private reset before accepting traffic.

Clock observation is its own original durable private write and can precede a
rejected data operation or invalid credential. Same-second checks are readonly;
closed admission never advances it. There is no atomic transaction spanning private
clock observations and public row commits. Original write poisoning, limits,
commit-before-return and uncertainty apply; never automatically retry a lost result.
No new stored or wire format is introduced.

## Verification plan

Stable/minimum Rust: new native scenarios for legacy/closed refusal before work,
current user auth/session/project scope, own/hidden CRUD and bounded pages, late
packet rejection, suspend/resume/current epoch/time/key rotation, authenticate-before-
data ownership, current policy/schema and malformed inputs, nonempty verified clone,
serialized concurrent users and independent generated owner/admission models.
Controlled staged/caller-received-result process kills exercise both original WALs
without supplying any service key to the child operation. Rerun original owned-row
checks, current private HTTP transport and direct project capability/isolation tests.
Strict workspace/fuzz formatting/linting and minimum compilation remain required.

No public HTTP route, wire parser, CORS/cookie/signup policy, role, user SDK/dashboard
or production milestone is enabled. These need separate verified increments after
this native boundary is accepted.


## Final verification

Rust1.99 and1.89 pass120 checks each: retained root49, existing private HTTP40,
actual account network12, original HTTP7, project capabilities9, registry model2
and server documentation1. Nine regular native cases are new, with24 generated
owner/admission sequences and four new process kills per toolchain, two after
the native caller actually received its result. Both original WAL formats and a
nonempty verified common-root copy are covered. No service key is passed to the
new child row operation. Original trusted service routes retain their checks.

Workspace/fuzz formatting and strict Clippy, minimum workspace build and all-target
fuzz compilation pass on492 frozen source/dependency/OpenAPI hashes. No new parser,
format or sanitizer campaign is claimed. The block adds839 Rust lines; total104434
source lines, Rust100147 (96324 effective), SDK2502 and Python1785. See
[source-bound evidence](../measurements/2026-10-09-native-public-user-gateway/verification.json).
Public network admission, operator CLI, signup, roles, user SDK/dashboard,
resource/load/upgrade and independent security/production gates remain open.
