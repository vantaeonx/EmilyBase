# ADR0117: scoped object envelope and original owned publication

Status: accepted for experimental synthetic-data use.

## Context

Private files need a bounded independently checkable representation before HTTP
uploads, signed downloads or backups can depend on them. Filenames and checksums
cannot grant user authorization. The original page-file publisher already retains
filesystem ownership across private staging, file sync, no-replace selection and
parent sync. A second publication mechanism would duplicate that contract.

## Decision

Add a synchronous object-storage crate with distinct 16-byte project/object IDs,
an exact experimental 96-byte header and an 8 MiB payload bound. Payload SHA-256,
header CRC32, strict lengths/flags/reserved fields and trusted expected scope must
pass before exposing a borrowed verified view. It is not user access authority.

Factor a bounded byte publisher around original storage Pending. Preserve page
creation and all database/WAL formats. Reuse retained directory, exclusive 0600
stage, file fsync, identity admission, no-replace rename, parent fsync and uncertain
result semantics. Reread original bytes before selection; final object inspection
precedes success. Post-selection errors do not imply rollback or delete a target.

Add readonly native inspection and a metadata-only offline CLI. Require expected
IDs, refuse aliases/nonregular/public/multiply linked files, bound allocation/read
and recheck change indicators. Paths remain operator authority. Add no empty HTTP
handlers or unimplemented adapters represented as working features.

## Verification and consequences

Final evidence will record both Rust toolchains, strict format/lint, original
storage regression/fault cases, actual CLI, generated data and decoder ASAN.
Received-result object process kills supplement the existing raw page-file matrix;
they do not prove power-loss durability. Initial test compilation exposed a missing
Unix metadata trait import, corrected before the final frozen run; no storage
corruption was claimed from this fixture compilation error.

HTTP authentication, project object-directory ownership, policies, quotas,
inventory/delete/cleanup, signed URLs and backup/restore remain separate work.
Objects are not part of currently verified root backups. Same-UID administrators
and selected ancestors remain trusted. Checksums are not encryption/authentication;
final inspection is a snapshot, not a retained lease. Larger streaming objects need
a separate bounded design. No production acceptance criterion is closed here.


Executed47 Rust checks each on stable1.99/minimum1.89.0:13 object,31 storage
unit/integration checks and3 actual CLI checks. Two ignored harness workers
are explicitly spawned by parents, not omitted scenarios. Eighteen new regular
cases include192 generated object draws, two received-result object kills and four
shared sync-failure injections per toolchain; the original three raw creation
kills rerun. Workspace/fuzz format and strict lint, stable CLI/server build,
minimum workspace build and all-fuzz compilation pass on539 frozen source hashes.
ASAN executes7,855,935 inputs in46 seconds without findings, RSS424MiB
under512,468 initial seeds/max input262144. The32-byte scope prefix and96-byte
header leave at most262016 fuzz payload bytes; the8 MiB bound is tested separately.
See [verification](../measurements/2026-10-09-object-files/verification.json).
