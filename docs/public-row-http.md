# Admitted public typed row HTTP

Only explicit account-root server mode exposes these routes. Users must obtain
their own current access token through [public sessions](public-session-http.md).
An operator provisions the schema and intended policies, explicitly migrates to
v5 and opens admission using the [offline command](admission-cli.md). Missing
policy or closed admission denies. Use synthetic data; production is not claimed.

All calls are POST under /v1/projects/PROJECT/user/rows. Require exactly one
Authorization: Bearer current access token and application/json. Service/master
keys, refresh tokens, foreign-project tokens and X-EmilyBase-Access alone cannot
authorize these routes. Existing trusted service+user /auth/rows calls retain their
original credentials. User credentials cannot call SQL/admin/user-control routes.

| Route | Outer JSON object | Result |
| --- | --- | --- |
| get | table, typed key | row or null |
| page | table, typed after or null, limit1..128 | visible rows and next visible key or null |
| write | table,1..256 typed operations | changed count and exact decimal transaction string |

Write operations are insert with a typed row, update with typed key/row, or delete
with typed key. Reuse the [original lossless wire](user-row-http.md): signed i64
values/keys and returned transaction identities preserve their full decimal strings.
Bytes are bounded integer arrays. Row columns remain positional typed arrays
**inside** the outer request object; positional outer requests are rejected.
Duplicate/unknown fields, inappropriate types, overflow and caller time refuse.
Bodies are limited to64KiB and five seconds; original row/record/value/packet/output
limits also apply. No arbitrary SQL, DDL, unfiltered scan or migration ledger exists
in this interface. Me supplies the current user's identity; any owner field in a
write is still checked against the current native principal by the installed policy.

The middleware checks known current enabled admission before body work. The shared
blocking root rechecks it after body/root waits before decode and trusted time.
The native operation verifies current access before opening public data or looking
up table metadata, then derives the current policy from actual table/schema identity
under both real owners. Current USING/CHECK decisions apply to every packet entry.
A later rejected entry discards the whole staged packet. Two competing primary
inserts have one committed winner. A policy or credential changed during body wait
applies before any public commit.

Hidden and absent reads both return row null. Pages include only visible rows/keys;
hidden lookahead does not become a continuation token. Missing policies, forbidden
operations and primary conflicts refuse through the original static failure map.
Write refusals do not promise complete secrecy of global primary-key occupancy.
No rejected packet partially changes its rows. Original time observation can be
a separate private commit even if a later operation refuses; closed admission
refuses before clock work. Same-second local checks verify unchanged private and
unrelated project WALs; network tests do not assume a frozen wall clock.

These routes share the original four workers,30 attempts per admitted private
project/minute and120 per actual socket peer/minute with private/user sessions.
Forwarded IP headers are ignored. Responses are no-store; logs use static matched
route shapes/codes and never credentials, body contents or project IDs.

Closing suspends user operations without revoking families. Intentional reopen can
resume a current unexpired session. Verified copy additionally closes admission
and changes incarnation before publication, so old source tokens stay invalid
after reopening; fresh login accesses preserved rows/policies. Source can continue.

A response may be lost after a complete write commit. Do not blindly replay the
packet: it can conflict or repeat a non-idempotent update. Use trusted inspection
and application-specific recovery. Received-response and observed-unread process
kills cover both WALs; they do not prove machine power-loss durability.

See [OpenAPI](openapi.json) and [ADR0114](adr/0114-admitted-public-user-row-http.md).
Signup, roles, user SQL, user SDK/dashboard, browser CORS/cookie behavior,
external TLS/deployment, load/upgrade/resources and independent security acceptance
remain pending.
