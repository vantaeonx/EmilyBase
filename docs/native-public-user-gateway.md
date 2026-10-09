# Native admitted user operations

The synchronous Rust AccountRoot now has user operations that require no project
service key. This is a native library boundary; existing HTTP routes still require
their documented service credentials. No user-only network route is implemented in
this increment. Use synthetic data; production readiness is not claimed.

An operator must explicitly migrate the private store to the
[v5 admission catalog](public-admission-catalog.md), install the intended row policies
and enable its current flag using a current service credential. Missing v5 or a
closed flag denies every public method before password/time/public-data work.
Opening a root does not implicitly migrate or enable any project.

| Native method | User credential | Result |
| --- | --- | --- |
| public_sign_in(project, login, password, trusted_now) | original exact password bytes | original access/refresh owners |
| public_refresh_session(project, refresh, trusted_now) | current refresh token | new single-use access/refresh pair |
| public_logout_session(project, refresh, trusted_now) | current refresh token | original durable logout |
| public_user(project, access, trusted_now) | current access token | that user's own AccountInfo metadata |
| public_user_table(project, table, access, trusted_now, operation) | current access token | bounded rows/page or commit metadata |

Service keys and refresh tokens do not authenticate access-only calls. Current
account identity, disabled/epoch state, project/incarnation scope, family state,
purpose, secret and deadlines are verified through original session methods.
Metadata returned by public_user does not grant authority on later requests.
No caller-selected user identity or copied admission receipt replaces a token.
Time remains an operator/service input; an eventual HTTP adapter must supply its
own trusted clock after any request wait.

Rows use the same [typed owned operations](user-row-enforcement.md) and
[visible keyset pages](user-row-pages.md). Authenticate before opening public data
or looking up table metadata, then derive current schema/table identity and the
installed policy proof inside the actual data ownership scope. Keep the private
owner borrowed through the complete operation. Missing policy denies even when
an existing trusted service route could access the same table. Hidden reads and
absent rows both return no row; page continuation exposes only visible keys.
Late conflict or policy rejection discards the complete staged write packet.
Migration ledger access, arbitrary SQL, DDL and unfiltered scans are not provided.

Closing the flag suspends login, refresh, logout, metadata and row access. It does
not revoke all families: an intentional reopen can resume an unexpired current
session. Use existing trusted epoch/session revocation when that is intended.
Service-key rotation does not revoke a current public user session. Changes in
admission, credentials, schema or policy apply on the next call; a multi-request
snapshot is not retained.

Verified nonempty clone closes admission and changes session incarnation. An
operator must explicitly reopen it and users must log in again. Old source tokens
stay invalid in the copy after reopening; source sessions and histories remain
unchanged. Generic engine restoration is a separate lower-level operation and
requires the documented private reset before traffic.

Original trusted time observation can commit separately even if an active user's
row operation later refuses. Same-second tests verify readonly failures, and a
closed flag refuses before clock work. Data commit and private clock observation
are not a cross-database atomic transaction. Storage uncertainty requires explicit
inspection, never blind retry. Native process kills do not prove power-loss safety.

The internal project selector is crate-private and its original capability is
consumed locally; it is never returned to a user. Existing service-key APIs preserve
their authority. Public HTTP, signup, roles, client SDK, dashboard, load/upgrade and
independent security acceptance remain pending. See
[ADR0111](adr/0111-native-admitted-user-gateway.md).

Operators can use the [offline admission CLI](admission-cli.md) for explicit
catalog migration, current metadata and CAS opening/closure. The root must be
stopped; keys remain in private files. This does not add a user-only network route.

The subsequent [public session HTTP adapter](public-session-http.md) exposes only
the four admitted auth/session/own-metadata operations. Typed user rows are still
native or trusted service+user HTTP; their user-only network adapter remains pending.
