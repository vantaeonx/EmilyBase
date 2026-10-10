# ADR0133: explicit Pager file-lock ownership

Status: accepted for experimental synthetic-data use.

## Context

Pager's private File can have an unexposed duplicate open description during
process creation. Merely closing the owner's descriptor can prolong its kernel
lock beyond the intended owner lifetime. A deterministic safe duplicate model
reproduces Busy after the original owner has ended. Native object directory owners
already explicitly end their own lock lifetime; Pager needs its own guard.

## Decision

Wrap each successfully acquired Pager File in private LockedFile with best-effort
explicit unlock on drop. Retain the guard from acquisition through all failed
constructor paths, or transfer it into the successful Pager. Create's existing
private duplicate moves before lock acquisition; duplication remains prepublication.
Do not expose the raw File or clone the owner. Preserve active exclusion, original
owned publication/sync/unknown outcomes, poison behavior, stored bytes and public API.

## Consequences and acceptance

Drop cannot report unlock errors; ordinary descriptor closure remains the fallback.
This native Pager lifetime is distinct from WAL/database/directory owner protocols.
No file compatibility, transaction acknowledgement, hardware power-loss or
production gate changes follow.

Require the failing create/open owner duplicate model to pass after the fix, keep
active competing opens denied, and prove closing the old duplicate cannot unlock
its successor. Cover inherited descriptions on failed length/header opens, original
before/after file/parent sync construction outcomes, poisoned-write refusal, exact
page preservation and later independent writes. Run storage/native/CLI plus affected
database/WAL/transaction/backup tests on both supported toolchains and frozen hashes.
See [contract](../pager-owner-lock-lifetime.md).

The original duplicate model fails with Busy before this correction. Final
stable/minimum each pass534 affected checks on583 frozen hashes, including four
new regular cases and321 dependent-core cases. Existing WAL last-clone semantics
remain tested and unchanged. The contract records scope, fallback, exact failed
result and limits; no full-source, parser sanitizer or production gate closes.
