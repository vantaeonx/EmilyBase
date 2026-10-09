# ADR0115: explicit caller-owned user TypeScript client

Status: accepted for experimental synthetic-data use.

## Context

The original Rust private-root gateway now exposes admitted user sessions and
typed policy-enforced row operations. The existing EmilyBaseClient accepts a
privileged project service key and exposes SQL/migrations. Giving that key to an
application user would cross the implemented authority boundary. A client must
transport actual user operations without taking over durable session decisions.

## Decision

Add a separate EmilyBaseUserClient accepting origin/project/fetch only. Expose
signIn, refresh, logout, me, rowGet, rowPage and rowWrite. Every protected call
takes an explicit access token; refresh/logout take an explicit refresh token in
JSON with no Authorization header. Tokens retain exact original purpose, family
and lowercase-hex shape. Structural token validation grants no authority.

The client stores no password/session/token and has no automatic refresh, retry,
cookie or browser-storage behavior. It rejects a service-key constructor option.
Returned plaintext pairs are explicitly caller-owned; the SDK cannot prevent an
application from serializing them or erase copies from the JavaScript heap.

Reuse the original exact typed-row validators and extract the original bounded
response reader for both clients. Session bodies/replies cap at4096 bytes and row
bodies/replies at65536. Passwords preserve exact valid Unicode UTF-8 bytes without
trimming/normalization, with1..1024-byte and escaped body limits. Integer metadata
uses canonical decimal strings. Unknown response fields/variants refuse. Session
pairs must share the same family metadata; this is shape checking, not scope proof.

Keep all original Rust storage, WAL, policies, current admission and session-clock
operations authoritative. A remote refusal has conservative unknown outcome because
the private trusted-time floor can commit separately even when the requested row
packet refuses. Local validation/cancellation before dispatch is not_started.
Lost/malformed replies never trigger an automatic second write or single-use
refresh. The privileged service client retains its existing outcome contract.

Fetch uses POST, no-store, credentials omit, redirect error and no-referrer. Errors
contain fixed known codes or generic failures, never arbitrary peer/transport text.
Closure denies future calls and does not cancel already dispatched operations.
Browser CORS/cookie/TLS deployment and actual browser/device verification remain
separate gates; no browser access is claimed from Node checks.

## Regression evidence

Initial tests reproduced two new-client defects before correction: an undefined
access token dispatched instead of failing locally, and a caller's later mutation
of its operation-array length changed response-count validation after dispatch.
Protected routes now always validate access, and receipt checking binds the copied
packet count. Another test reproduced an untyped failure for invalid request-options
objects; these now refuse with the fixed local typed error before fetch.

## Verification and limits

Final commands, immutable source hashes and executed counts are recorded in the
increment's measurement after completion. Unit tests include generated Unicode
boundary cases, bounded streamed replies, purpose/shape/clock validation, input
snapshots, aborts and conservative failures. Actual Node Fetch talks to the Rust
server/CLI on stable and minimum Rust using both WAL versions, two isolated users,
received-result SIGKILLs, explicit lost-response adapters, policy rollback, verified
nonempty restore and offline credential revocation.

The lost-response adapter deliberately consumes an actual server response then
throws before the SDK receives it. The fixture observes a complete committed
operation; this is not an arbitrary pre-confirmation kill or power-loss proof.
The existing external legacy-container probe skips private-root SDK provisioning;
local/hosted native-binary checks run it. No npm publication, registration, roles,
user SQL/DDL/migration API, persistent session manager, realtime, objects, dashboard,
Kotlin, independent security audit or production readiness is added.


Executed: Node22.22.1 strict TypeScript compilation and33 unit tests pass, with
10 new unit scenarios and128 generated exact-Unicode cases. Original and private
native-binary integrations pass12 checks each on Rust stable1.99 and1.89.0, without
skips. Each binary run includes two new WAL scenarios and their parent, six new
received-result SIGKILLs and four deliberate observed-lost responses. SDK format
checking, Rust format checking and both actual CLI/server builds complete. No Rust
source changed and no new sanitizer/fuzz run is claimed. See
[measurement](../measurements/2026-10-09-explicit-user-sdk/verification.json).
