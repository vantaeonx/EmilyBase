# Owned user-row enforcement

`AccountRoot::user_table(project, service_key, table, access, trusted_now, operation)`
is a synchronous trusted gateway to one actual public table. Both a current project
service key and a current user access token are required. Never put a service key
in a browser/mobile client. A separate [trusted backend HTTP adapter](user-row-http.md)
requires both credentials; user tokens alone grant no SQL or data route.

The root derives the table identity and complete schema under the original public
data gate/database owner. It then verifies the currently installed policy and
current session under the retained private owner. Both owners remain held through
the decision and original commit. A caller cannot supply table identity/schema,
forge a policy proof or retain a detached capability. Missing/disabled catalogs,
missing policy, wrong scope/schema and invalid/revoked credentials refuse.

| Operation | Decision and result |
| --- | --- |
| Get(Key) | Check exact context even for an absent row; copy only a SELECT-permitted row. Absent and SELECT-denied both return Row(None). |
| Page { after, limit } | Return 1..128 permitted rows in primary-key order; continuation uses only a visible key. See [page semantics](user-row-pages.md). |
| Write(Insert(Row)) | Check the complete candidate against INSERT, then stage it. |
| Write(Update { key, row }) | Load the actual staged old row, check UPDATE USING and candidate CHECK, refuse primary-key changes, then stage it. |
| Write(Delete(Key)) | Load the actual staged old row and check DELETE before staging it. |

A write packet contains 1..256 operations on that one table. Later operations see
earlier staged writes, including an inserted row. Any denial, invalid typed/physical
row, missing update/delete target, duplicate key or late constraint error drops
the complete transaction. UPDATE/DELETE do not acquire an implicit SELECT condition.
A successful packet commits once through the original WAL/fsync path and returns
its actual u64 transaction plus accepted operation count. Known migration-ledger
names are excluded case-insensitively even if a policy would allow them.

Native typed inputs obey existing catalog/page bounds. This API does not bound
allocations already made by its trusted caller; public request admission is a
separate pending gate. Request/result Debug is redacted. No arbitrary callback,
SQL, DDL, unfiltered scan or private database handle is exposed.

Current policy replacement, password/epoch changes, disable, refresh/logout and
restored incarnation apply on the next verification. Drop/recreate yields a new
table identity and requires a new installed policy. A stale complete schema refuses
even if the requested row is absent. Verification can separately commit a forward
private time watermark before public work. A rejected public write cannot roll
that observation back; there is no cross-database transaction. Equal trusted time
leaves private history unchanged for reads and denied public packets.

No automatic retry or idempotency receipt is provided. A storage/commit failure or
lost result requires inspection; it must never be interpreted as rollback. Returning
no row masks result contents/existence, not timing, all key/constraint side channels,
or whole-process resource usage. Service SQL keeps its separate trusted authority.

Local checks exercise both original WAL versions, independent generated row models,
late rollback, current policy/session/project/schema boundaries, simultaneous
public/private owner retention, competing same-key writers and controlled process
kills before commit and after received results. See [verification](measurements/2026-10-09-owned-user-row-enforcement/verification.json)
and [ADR0104](adr/0104-owned-user-row-policy-enforcement.md). Native [filtered continuation](user-row-pages.md) is implemented separately.
Public user-only admission, roles, wider security/load/upgrade/resource and production
gates remain open. Only synthetic data is used.
