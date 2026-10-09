# Admitted public user session HTTP

Only explicit account-root server mode exposes these user routes. The legacy
registry router has no private user store. An operator must provision users,
explicitly enable policy/v5 catalogs and open current admission through the
[offline admission CLI](admission-cli.md). No startup migration or anonymous signup
occurs. Use synthetic data; production readiness is not claimed.

All routes are POST under /v1/projects/PROJECT/user:

| Route | Authorization header | Exact JSON body |
| --- | --- | --- |
| sign-in | absent | login and password string fields |
| refresh | absent | refresh_token string field |
| logout | absent | refresh_token string field |
| me | exactly one Bearer current access token | empty object {} |

Project/master keys do not authenticate these calls. Duplicate Authorization
headers refuse. A refresh token cannot act as access; user tokens grant no SQL,
project key rotation, user administration or unfiltered data access. Existing
trusted /auth and administrative routes retain their original key requirements.

Every body requires application/json, is limited to4096 bytes and five seconds,
and must be an object. Duplicate/unknown fields, positional arrays and caller time
refuse. Password bytes remain the original exact UTF-8 bytes,1..1024; the wire body
bound can reject heavily escaped representations even when decoded bytes fit.
No normalization occurs. Secret fields and the parsing copy are wiped by their
owners; transport/Serde internals may hold other copies, so whole-heap erasure is
not claimed. Me takes its access token only from the header, never the body.

Missing/legacy/closed admission refuses before body/password/time work. Only a
known current admitted project consumes the bounded project attempt map. The
original four worker permits are shared with trusted operations. Both namespaces
share30 private attempts per project/minute and120 per actual socket peer/minute;
forwarded IP headers are ignored. Pending bodies have a timeout, cancellation
releases their permit, and health remains available independently.

After body/root waits the adapter rechecks the flag before reading its own trusted
clock and invokes the original [native boundary](native-public-user-gateway.md).
That method verifies selected filesystem identities, current session purpose,
project/incarnation, family/account epoch, disabled state and deadlines under the
actual root owner. Refresh remains single-use. A concurrent refresh has one winner.
Time is not a field clients can select. An active user's original time observation
can commit even if its credential later refuses; this is not a readonly promise
for every invalid request. Closed admission does not observe private time.

Sign-in/refresh return access_token, refresh_token, token_type Bearer and exact
decimal-string expires_at. Logout returns logged_out true. Me returns only the
current user's id, login, decimal-string credential_epoch and disabled state.
No returned metadata is authority on a later call. All responses are no-store;
errors and matched-route logs contain static codes/shapes without credentials,
request bodies, project IDs or raw filesystem paths. See [OpenAPI](openapi.json).

Closing suspends all four calls, including logout, without revoking all families.
An intentional reopen can resume an unexpired current token. Use explicit trusted
revocation when required. Service-key rotation is independent of user sessions.
Verified common-root/private copy closes admission and replaces session
incarnation before publication. After explicit reopening, users must log in again;
source tokens stay invalid in the copy while the source can remain active.

A session mutation can commit before a response is lost. Never blindly retry a
refresh or infer rollback from a broken connection. Inspect through trusted
operator/session state and use the original recovery contract. Received-response
process kills test the HTTP boundary; they do not prove machine power-loss safety.

User-owned row HTTP, browser CORS/cookie policy, signup, roles, user SDK/dashboard,
external deployment/TLS, load/upgrade/resources and independent security acceptance
remain pending. This is a same-origin/native-client API increment, without a
cross-origin browser access promise. [ADR0113](adr/0113-admitted-public-session-http.md).

The subsequent [public row HTTP adapter](public-row-http.md) now uses current
access credentials for typed owned get/page/write through the same admitted scope.
It retains original table-policy/data ownership and adds no service-key fallback.
