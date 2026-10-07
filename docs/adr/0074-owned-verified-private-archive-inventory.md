# ADR0074: owned verified images and pure private archive inventory

Status: accepted for private inspection/export; combined capture and restore open.

## Decision

An archive may be valid at the page/WAL layer while violating private account
schema, project binding or family references. Reuse one complete private snapshot
validator for opening, pure archive inspection and explicit private export.
Inspect every component before a future combined restore can publish a platform.
No finished database engine or additional runtime dependency is introduced.

Backup decode_verified validates the existing EMILYBAK envelope, replays its
bounded acknowledged WAL and retains an owned VerifiedBackup image/report.
The image has private construction, immutable accessors and redacted Debug.
It contains a read-only original-engine snapshot without a filesystem owner,
credentials or authorization authority. It remains valid after dropping/changing
input bytes, changing the source database or removing its directory. Existing
inspect_bytes returns the same metadata and drops that image. No wire version
or acknowledged-prefix semantics change. A caller can retain many images;
per-format count/size bounds are not a whole-process numeric heap reservation.

inspect_private_account_backup_bytes validates an independently expected project,
then applies complete private versions1/2/3 inventory validation to that owned
snapshot. The report contains original database/commit/WAL counts, private version,
account count, all retained family count and optional clock floor. It exposes no
user identity, password verifier, session scope or token and grants no permission.
Historical, revoked and obsolete-scope families still count until explicit cleanup.
Inspection never resets a clock, changes a session, creates a directory or opens
a live database. Metadata correctness is distinct from password/token proof.

AccountStore opening now uses the same validator to reconstruct its private
state. File backup and backup_image validate private semantics before export;
backup_image explicitly returns sensitive archive bytes for trusted capture.
They retain password verifiers and plaintext private metadata, with no automatic
logging, serialization into a public route or encryption claim. Current-state
export verifies the acknowledged WAL against the live snapshot through the engine.
The caller remains responsible for private byte retention and authorized paths.

## Validation and compatibility

Exact version-selected schemas, scope/meta/clock cardinality,1024 unique accounts,
4096 retained families, fixed verifier policy, project/incarnation binding,
positive identity/epoch/generation fields, clipped deadlines and user/family
references retain their existing strict rules. Current-incarnation issue time
cannot exceed the clock; older-incarnation history after reset is allowed.
Unknown/partial inventories fail, even with recomputed engine integrity fields.
Readers do not silently migrate or reset input. Existing ordinary backup inspection,
restoration and private reset-before-publication retain their behavior.

## Evidence and next gate

Tests cover both WAL versions and all private versions, exact file/image exports,
source immutability, owned image lifetime and redacted formatting. Twelve
engine-valid/private-invalid fixtures fail through opening, pure inspection and
both export paths. Actual1024/1025 account and4096/4097 family boundaries execute.
An independent64-case inventory model varies account count, format, clock and WAL.
Revocation/disable/reset/pruning tests retain historical counts while checking
that metadata inspection does not revive or invalidate existing authority.

A new parser-only ASan target consumes raw archives and independently modeled
synthetic engine-valid account records. Structured inputs rebuild checksums to
reach semantic validation; opaque synthetic verifier bytes grant no password
proof. Six original-engine corpus seeds cover private versions1/2/3 and WAL1/2.
No live KDF/filesystem operation occurs in the target. Native early-refusal
observations cover invalid project and malformed envelopes without input-sized
copies; valid snapshot decoding is allowed to allocate. Actual results are in
testing.md and the source-bound artifact.

A future combined capture must own every project data database and private account
store before reading its first acknowledged prefix and retain all owners until
capture finishes. An existing registry backup image drops its temporary data
owners before returning; appending independently captured account bytes afterward
would not prove coordinated consistency. Scope/identity checks alone do not
establish a common capture boundary. Combined root publication must durably reset
restored private scopes before traffic. Neither that publisher nor HTTP account
workers, roles, row policies or numeric whole-process memory admission is enabled
here. No production/milestone gate closes.
