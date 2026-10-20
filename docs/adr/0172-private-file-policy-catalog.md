# ADR0172: explicit private v6 file-policy catalog

Status: accepted for experimental synchronous native library use.

## Decision

Explicitly migrate a fully valid private v5 store to v6 by creating the exact
auth_file_policies table and changing its scope version in one original commit.
Preserve users, session incarnation/clock, row policies and public admission state.
An empty file-policy catalog denies file access. V1..v5 remain readable without
automatic migration; v1..v4 must explicitly reach v5 before enabling this catalog.

Limit inventory to128 complete records. Validate exact schema/arity/native IDs,
canonical keys, bounded grammar/digest and revisions within confirmed history.
Install/replace one row using the actual next native commit ID and exact expected
revision. Accept identical retries only against the recorded current/predecessor
revision; unrelated commits cannot manufacture a retry or reopen an older policy.

Construct a current file-policy proof only inside AccountStore from validated
installed data and a genuine current session. Keep both under the same original
private store borrow. Export observations, never a cloned/serialized proof or a
detached native file handle. The future native adapter supplies actual selected
file context/reference ownership and consumes the proof immediately.

Teach complete private validation, session inventory, row-policy compatibility,
admission and native root admission about exact v6 table counts. Verified restore
preserves policies while changing session incarnation and closing public admission
in the original reset transaction. Do not change database/WAL/page/record versions.

## Acceptance

Both WALs: explicit/no-op migration, old-version refusal, independent CAS/retry
model, bounded inventory, real current proof and borrow exclusion, revoked tokens,
reopen/compaction, complete private corruption validation, verified independent
restore and actual staged/committed process kills. Exercise native common-root
open/capture/restore compatibility and existing row/admission behavior.

No HTTP/SDK file-policy administration or actual native user-file operation is
introduced by the catalog alone. Production acceptance remains open.
