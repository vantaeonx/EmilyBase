# ADR0136: native file metadata CAS and logical removal

Status: accepted for standalone synchronous native FileStore.

## Decision

Add trusted native rename/remove using the exact current reference revision as CAS.
Validate complete current catalog/inventory, retain the actual object reader across
staging, reverify before attempting commit, then check the committed present/absent
reference, quota, graph/inventory, original source and metadata ownership again.

Identical current display text is a no-op only after current revision matching and
verification. Rename preserves blob/owner and stores the actual commit revision.
Remove deletes only the logical row, returning previous metadata and deletion
revision. The physical blob remains private, immutable and charged. Missing deletion
is not an assumed idempotent retry. Reused logical IDs require fresh blobs/newer
revisions, preventing old commands from altering a later generation.

Before a commit attempt, source failure discards staging, returns a direct error and
requires reopen. After the attempt, errors are uncertain and poison FileStore.
Neither boundary repairs, replaces, cleans up or blindly retries. Lost responses
can leave committed changes; staged changes remain absent on recovery.

## Consequences

Version1 private schema, original formats, fsync and runtime routes remain. A payload
reader borrow prevents simultaneous mutable operations. Native WAL1 and compacted
WAL2 pass mutation/recovery checks, not only lower-level WAL cases. The existing
trusted native boundary remains; checks do not grant current account authority or
a permanent namespace lease.

Quota administration, content replacement, reclamation, retry identity, coordinated
backup/restore and actual Root/current-account enforcement remain open. See
[contract and executed evidence](../native-file-mutations.md) and the proposed
[root integration](0127-proposed-account-root-object-integration.md).
