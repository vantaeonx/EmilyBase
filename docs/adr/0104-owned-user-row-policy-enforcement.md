# ADR0104: Current user policy enforcement inside an owned original transaction

Status: accepted for the synchronous owned gateway after the checks below; no user HTTP route.

Add a synchronous retained-root operation for exact-key read or1..256 typed writes
on one actual public table. Require current project service key, trusted time and
current access token. Derive ID/schema under the original held data gate/owner,
then verify the current installed policy/session under its retained private owner.
Both remain held through decision and commit; no detached capability, arbitrary
callback, SQL/DDL, unfiltered scan or request-supplied context is exposed.

Exact-key reads copy only permitted rows. Absent and SELECT-denied rows both return
None; invalid scope/schema/row and invalid credentials remain errors. This masks
row contents/existence in result shape, not timing or all constraint side channels.
Writes check each current staged row: INSERT candidate, UPDATE old USING plus new
CHECK and stable primary key, DELETE old row. Later operations see earlier staged
writes. Any denial/invalid input/late constraint error discards the entire owned
transaction. Successful packets commit once through the original WAL/fsync path.
No automatic retry/idempotency receipt is added. Ambiguous storage outcomes require
inspection. Known migration-ledger names are excluded even if a policy allows them.

Session verification may separately commit a forward private time watermark before
public work. A failed public write cannot roll that observation back; this is not
cross-database atomicity. With equal trusted time, denial/read/retry changes no
private history. A policy replacement, disable/password/logout/refresh or restored
incarnation applies on the next verification. Table recreation requires a fresh
policy and schema changes refuse stale binding. Existing service SQL remains a
separate trusted authority; user tokens alone grant neither this gateway nor SQL.

Acceptance requires both-WAL owned CRUD/hidden reads, staged-row and late-denial
rollback, typed input/packet caps, current session/policy/schema/project boundaries,
actual simultaneous public/private owner retention, independent generated row model,
thread serialization, controlled staged/received-result kills and verified restore.
Strict stable/minimum checks must pass before acceptance. Filtered pagination,
roles, user HTTP DTOs/admission, schema/policy coordination, broader side-channel,
security/resource/load/upgrade/production gates remain open.

Frozen-source checks pass 39 regular cases plus one borrowed-proof compile-fail
case on each Rust toolchain: nine new owned-root cases and 30 adjacent auth/policy
cases. Each toolchain exercises 24 independently generated row sequences and four
controlled user-packet kills across WAL1/2, before commit and after received result.
Strict workspace/fuzz Clippy, formatting, minimum workspace build and all-target
fuzz compilation pass. An actual wrong-schema/absent-read regression first failed,
then passed after context validation moved before existence lookup. No new parser
or storage format is introduced; a new sanitizer campaign is not claimed.
See [source-bound evidence](../measurements/2026-10-09-owned-user-row-enforcement/verification.json).
