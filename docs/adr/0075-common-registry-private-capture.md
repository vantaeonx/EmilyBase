# ADR0075: common capture of registry and explicit private account roster

Status: accepted for offline capture/inspection; publication and restore open.

## Decision

Appending an account archive after registry backup_image has returned does not
hold data owners across both reads. Refactor registry capture into a scoped
callback that retains every original-engine data owner until private capture and
final registry checks finish. The existing standalone registry backup remains
byte-compatible and uses the same helper with an identity callback.

AccountStore owners already exist when passed in a mutable slice; the caller's
exclusive borrow prevents in-process mutation during capture. Require at most128
unique private scopes already present in the registry and refuse outstanding
AuthorizedProject capabilities. Acquire every data directory/WAL lock before
reading the first acknowledged prefix. Keep them through the complete callback,
including errors; Rust drop releases temporary data owners without transferring
or closing the caller's private owners. This establishes a common quiescent state
for cooperative original-engine writers across the explicitly supplied sources.
It does not impose a single transaction ID across independent databases or
protect files against a malicious local administrator ignoring locks.

Introduce the bounded canonical [EMILYBND-1](../account-bundle-format.md) envelope,
containing unchanged EMILYREG-1 and private EMILYBAK-1 bytes. Exact lengths, sorted
unique scopes, shared database identity uniqueness, existing complete WAL replay
and private versions1/2/3 schema validation are mandatory. Inspection is pure:
no files, migrations, session reset, password check or request authority. Report
only registry metadata and private inventory counts; no private row contents,
verifier, user identity, session incarnation or token is formatted in reports.

A roster may intentionally omit private stores. No attached-store catalog exists
yet, so never label the supplied subset as every platform service. The encoder
canonicalizes order without changing the caller's slice. Per-image and total
encoded limits are checked with overflow-safe arithmetic; a new private image
is checked before retention. Individual replay/source/output allocations still
exist. These format caps are not numeric whole-process heap admission.

## Security and compatibility boundaries

Bundle bytes are sensitive plaintext, including current key/password/session
verifiers. SHA-256/CRC are integrity checks, not authenticity or provenance.
Capture and inspection do not invalidate old API keys or sessions. Existing
registry key digests/epochs remain unchanged; changing epoch metadata alone does
not rotate a hash-only API key. Private restore must use the existing reset-before-
publication wrapper, and a future combined root publisher must preserve that gate.
No HTTP route consumes bundle bytes or private operator-selected paths here.

The original registry standalone format and backup behavior remain unchanged.
No ready database engine, new runtime dependency, implicit format migration or
production acceptance is introduced. The own WAL crate is used only by new tests
to construct semantically valid identity-alias archives.

## Evidence and next work

Tests compare exact nested source images, canonical reversed-roster encoding,
versions1/2/3 and WAL1/2, empty/subset inventories and public metadata redaction.
Boundary callbacks attempt competing data/private opens before the first prefix
and after a private prefix: every owner remains held. Refused capabilities,
competing owners, duplicate/foreign roster scopes, changed final metadata and
corrupt private capture preserve source selections and release temporary owners.
An independent32-case mutation model checks data/account/time inventories.

Six forced-process-termination cases cover both WAL versions at all-data-owner,
private-prefix and returned-capture boundaries. They check exact acknowledged
source bytes, cross-process contention before capture completion, restart and
owner release. This is read-only capture evidence, not a new commit durability,
fsync-failure or physical power-loss experiment. Malformed length/version/checksum,
trailing payload, sorted duplicates, foreign project and valid nested identity
aliases are rejected. A parser-only ASan target consumes raw and repaired outer/
registry envelopes seeded by explicitly generated original-engine bundles.
Actual final checks and counts are recorded separately in testing.md.

Next: privately publish this envelope without replacement; restore all selected
data/private stores into one owned root; validate/reset every restored private
scope before root publication/traffic. An authoritative private roster, private
worker/rate admission, account policy, roles/RLS, numeric resource reservations and
production/crash/upgrade/security acceptance remain open.

Follow-up: [ADR0076](0076-owned-account-bundle-file-publication.md) implements owned
private file publication and count-only CLI verification. Combined restore remains open.
