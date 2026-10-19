# ADR0168: outer coordination at actual native file write boundaries

Status: accepted for experimental synchronous native coordination.

## Context and decision

The readonly project gateway cannot safely become a mutation gateway by checking
current authority only before and after a call. File publication selects a durable
blob before a metadata WAL commit; late failures have materially different outcomes.

Expose four guarded NativeFileRoot entry points and a copied NativeWritePhase enum.
Run trusted checks at actual existing BeforeBlob, BeforeCommit, Committed and
Verified boundaries, plus BeforeOperation admission. Keep internal mutable database
and selected blob handles private. Recheck native selection after a successful
callback and preserve the existing final state proof. Keep unguarded APIs on their
previous path. No async storage, dependency or file-format change is needed.

## Consequences

Callbacks must supply outer current authority; native callbacks are not permissions.
Selected payloads and original actual writer owners remain held across their existing
proofs. Metadata/quota no-ops have no invented commit. Ordinary preflight denial,
staged metadata poisoning, blob-first uncertainty and committed uncertainty retain
their distinct meanings. Preserve physical objects and confirmed metadata; do not
claim rollback after publication. Poison on trusted callback unwind and propagate
the original panic, preventing a caught panic from resuming a partially observed owner.

Verify both WALs, actual phase/WAL order and owner/reader lifetimes, errors at every
phase, source changes despite callback success, no-ops, invalid/CAS inputs,
generated recovery models, capture/restore and 54 actual signal9 kills per toolchain.
See the [contract](../native-file-write-guards.md). Native current-key/common-root
file mutation integration is deliberately a later logical change; user policies,
HTTP/SDK, deployment drills and production acceptance remain open.
