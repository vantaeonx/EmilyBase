# ADR0137: scoped native quota CAS with retained complete inventory

Status: accepted for the standalone synchronous native FileStore.

## Decision

Expose an opaque QuotaState containing expected limits, project, metadata database
identity and global transaction revision. Trusted native operators change limits
through exact scoped global CAS. Other database identity refuses; any intervening
reference/quota commit invalidates an old state. Current equal limits are a no-op
only after matching CAS; stale equal requests conflict.

Require the proposed limits to fit complete physical inventory, including orphans,
before staging. Retain and fully verify every actual physical object's readonly
descriptor under the original owner through scope-row staging, precommit checks,
the original WAL commit and final unchanged-reference/graph/source checks. Bound
these retained file descriptors by the existing128-object inventory cap. Partial
admission failure drops the acquired set and writes nothing.

Precommit source failure discards staging, returns a direct error and requires
reopen. Errors after the commit attempt are uncertain and poison FileStore. No file
is removed, replaced, repaired or excluded from physical accounting. The existing
scope row's limit values change without a new schema field/version or format.

## Consequences

Quota settings are now explicitly mutable persisted operator state. The copied CAS
state grants no current account authority, resource reservation or unique-ancestry
proof for identical restored copies. Global CAS is conservative but avoids a new
quota-revision migration. Logical deletion still leaves charged physical storage.

Repeated full hashing and at most128 extra native descriptors are a bounded local
cost, not a throughput claim or global server admission. Native filesystem trust,
whole-process budgets, current file policy, Root integration, common backup/restore,
reclamation, request idempotency and production acceptance remain separate gates.
See [contract and evidence](../native-file-quota-administration.md).
