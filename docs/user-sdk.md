# Explicit user SDK contract

The [TypeScript package](../sdks/typescript/README.md#explicit-user-client) now
transports original Rust admitted sessions and typed rows through a separate user
client. It requires an explicitly opened private v5 project and existing users and
table policies. It does not provision users or install policy definitions.

Every protected operation takes its access token explicitly. Refresh/logout take
only a refresh token in JSON, without an Authorization header. No service/master
key, private trusted context, client time or arbitrary SQL is accepted. Sessions
remain owned by the original Rust engine. The SDK validates shapes and transports
requests; it is not a database engine or authorization authority.

Passwords are exact valid Unicode strings whose UTF-8 bytes are1..1024, subject
also to the4096-byte escaped JSON body cap. Byte passwords that are not valid UTF-8
cannot be represented through this JSON client; the offline native provisioning
contract remains separate. No trimming or normalization occurs. Unpaired UTF-16
surrogates refuse before TextEncoder replacement. Session replies and metadata cap
at4096 bytes; row payloads/replies use the original65536-byte bound. All wire integer
metadata and typed row integer values retain canonical decimal-string precision.

Caller-owned token pairs are returned explicitly and are not held by the client.
Do not serialize a pair into application logs. Client serialization exposes only
project/closed metadata. The SDK cannot wipe immutable JavaScript strings or
application copies, and makes no memory-encryption promise. It never installs a
storage handler, sends cookies or automatically refreshes/retries a request.

not_started denotes a locally rejected call. Once dispatched, remote refusal,
disconnect, abort, malformed response or uncertain storage result is conservative
unknown. Even a rejected row operation can separately advance trusted private time;
the SDK does not claim that every WAL is unchanged. Current server400/403 refusal
still prevents a partial row packet. Losing a successful single-use refresh result
may require a fresh sign-in; do not automatically reuse the old refresh token.
Losing a write result requires application-specific inspection before a retry.

rowWrite accepts1..256 operations for one table and checks the response count against
the immediate copied input packet. Hidden reads/pages retain original policy
semantics. Primary-key write conflicts can reveal that a key is unavailable; this
does not claim complete timing or existence noninterference. Pages observe current
state, with no multi-request snapshot. Schema/record limits remain Rust checks.

Node22 tests use real Rust binaries on stable/minimum toolchains and both WALs.
The existing legacy external-container SDK probe skips private-root provisioning;
it is not evidence for this new private-root workflow. Node tests do not establish
browser/mobile compatibility. The current server still has no browser CORS/cookie
contract. Signup/roles, persistent session orchestration, realtime/files, dashboard,
Kotlin, npm release and production acceptance remain pending. See
[ADR0115](adr/0115-explicit-user-typescript-client.md).


The subsequent [own-password change](user-password-change.md) is also transported
as changePassword(access, currentPassword, replacementPassword, options). Native
Rust derives the current account, verifies the old password and commits the new
digest/epoch; the client only sends the exact bounded fields and validates UserInfo.
Successful change revokes all old families and requires explicit fresh sign-in.
No password recovery, administrative reset or automatic retry is added.
