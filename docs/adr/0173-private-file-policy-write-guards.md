# ADR0173: trusted checks at actual private file-policy write phases

Status: accepted and verified as an experimental synchronous native library.

## Decision

Expose guarded variants of the original private file-policy migration and install
methods. Use trusted callbacks at BeforeOperation, BeforeCommit, Committed and
Verified. BeforeCommit occurs after bounded native staging and before the actual
original commit; Committed occurs only after that commit returns successfully.
No-op calls use BeforeOperation/Verified only. Unguarded APIs retain the same bytes,
I/O and recovery behavior through the original path.

A callback refusal before commit drops unconfirmed stages and returns an ordinary
typed error. A refusal at Committed/Verified after successful commit withholds the
receipt and reports typed outcome uncertainty; committed metadata is preserved.
No rollback, cleanup or automatic retry is invented. Native storage commit errors
retain their original typed cause. The future common-root coordinator must track
that an accepted BeforeCommit can be followed by a native write attempt.

Callbacks are trusted native coordination, not principals or authorization grants.
The actual AccountStore owner remains borrowed through every phase. Callback panic
unwinds normally; pre-commit staging drops, while completed commits remain durable.
An enclosing common-root adapter must handle possible publication on both returned
errors and unwind, and close its whole live surface until explicit inspect/reopen.
The raw private store does not pretend to own that external namespace contract.

## Acceptance

Both WALs, migration/install/replacement, exact/no-op/conflict behavior, refusal at
each reachable phase, original owner lifetime, callback unwind, real staged/committed
signal9 kills and recovery followed by a separate write. No persisted schema/format
or public HTTP/SDK change; the guarded API alone does not implement AccountRoot
file-policy administration or actual user-file enforcement.

## Common-root integration

Lend disjoint original native/private owners from the existing current-key file
frame. Missing private ownership refuses without provisioning; existing v2 manifests
still require private membership for each native file binding.
Policy administration requires that actual private store and derives nativeDB from
the internally owned file root; never accept an asserted database identity.

Repeat original common/key checks at actual private phases. Only mark a possible
write after accepting BeforeCommit. Any subsequent returned error, including a
native commit error or the enclosing final check, poisons the whole Root and keeps
a typed AccountFilePolicyWriteUnknown cause. Preserve metadata until explicit
inspect/reopen. A no-op never invents publication. Catch trusted unwind around the
whole frame, set the same poison after possible publication, then resume it.
No extra public-data writer lock or source reopen is needed.

See [contract](../file-policy-write-gateway.md). File-policy native operator metadata
is distinct from current user-file enforcement and supplies no public HTTP route.

Retain a readonly private WAL selection descriptor, established against the actual
AccountStore directory/WAL under the same frame. Its inode must remain selected at
all private phases and through final enclosing checks. Allow legitimate original
append size/timestamp changes; never adopt an equal-byte replacement. This is a
finite original selection proof, not a new directory/writer lock or namespace lease.
Both native and private WAL replacement regressions must fail before their fixes.
