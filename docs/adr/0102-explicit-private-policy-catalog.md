# ADR0102: Explicit private v4 policy catalog and current borrowed policy proof

Status: accepted for the local catalog after the checks below; no end-user HTTP subsystem.

Add an explicit v3-to-v4 original private-store migration creating the exact policy
header/chunk tables and advancing auth_scope version in one original WAL commit.
Do not implicitly upgrade existing roots or change default root initialization.
v1/v2 must first explicitly enable the existing v3 clock. v4 migration preserves
users, verifier policy, session incarnation/families and clock. Complete v4 open,
archive and restore validation includes every policy/header/fragment and rejects
unknown schema, orphan fragments, invalid nested definitions or future revisions.
Existing v1..v3 remain readable; older readers refuse v4 rather than dropping it.

Cap installed table identities at128 and fragments at seven each. Stream complete
headers/chunks under normal original row/table/page/WAL limits. Validate exact
project scope, group checksums/schema/definition bounds and revision<=private last
committed transaction. Policy revisions use actual next private commit IDs and
may have gaps from unrelated private writes. Inputs/contexts come from the trusted
caller, never request-provided assertions about target table ownership.

Expose local install/list and a current-policy access proof. Installation requires
expected current revision (0 for absent); identical current definition/schema
with current or recorded predecessor expectation returns the existing receipt
without a write. Changed content requires current revision and one complete atomic
header/chunk replacement. Persist predecessor for explicit lost-result retry;
no delete/recreate revision reuse or automatic retry. Disable by installing deny
rules; bounded obsolete-table cleanup is a separate tombstone/ABA design gate.

Load/validate the installed model before verifying the current access session,
then return a non-cloneable borrowed PolicyPrincipal holding both current session
proof and immutable model. Its fields are private; only exact-context row decisions
are exposed. Borrowing the private owner prevents policy/credential replacement
while that proof lives. Missing/disabled catalogs and missing policy grant no
row authority. End-user routes stay closed until trusted public-data gate/context,
transaction enforcement, filtered reads and request admission are integrated.

Backups/root bundles preserve v4 policies through the existing opaque original
private archives. Restore still resets session incarnation/time; policy rows/revisions
remain exact, old tokens remain revoked and explicit fresh sessions use current
policy. No core database/WAL codec or bundle container version changes. Default
v3 roots keep their existing semantics. New methods/errors redact definitions and
propagate ambiguous original commit outcomes without inferred rollback.

Acceptance requires exact no-op/failed/CAS histories, whole-inventory corruption
and orphan/refuture checks, proof lifetime/current-policy revocation, independent
install/restart models, WAL1/2 staged/received-ACK kills for migration/first/replace,
verified private/common-root restore with nonempty policy groups, current server
scope regression, strict lint/MSRV/property/fuzz checks. Do not mark this ADR
accepted until those criteria actually pass. Passing this catalog gate does not
enable a user route; public data enforcement requires separate acceptance.

All listed local criteria pass on the frozen source set:345 auth/server cases per
Rust toolchain, including12 controlled catalog stage/received-ACK kills each,
32 independent revision sequences each and16 common-root restore combinations
each. Strict formatting/Clippy, minimum workspace/fuzz compilation and a45,260-run
ASan private-archive campaign pass. The reproduced extra-table list-validation
error was first demonstrated by a failing test, then corrected before final runs.
See [source-bound evidence](../measurements/2026-10-09-private-policy-catalog/verification.json).
Wider deployment/upgrade/security/power-loss/resource and end-user enforcement
gates remain open. No platform milestone is marked complete.
