# Native file write coordination

Experimental synchronous Rust integration primitive. These callbacks are trusted
coordination, not user permissions, HTTP middleware, a reservation or a filesystem
lease. AccountRoot file mutations and current project authority remain a later step.

NativeFileRoot exposes publish_guarded, rename_guarded, remove_guarded and
set_quota_guarded. They use the existing blob-first write, metadata transaction,
quota/CAS validation and original component owners. The older methods preserve
their existing path without an additional external callback or extra callback proof.

## Actual boundaries

The callback receives only NativeWritePhase; no database, blob descriptor or owner
is exposed. It returns the native Result type. GuardRejected provides a static
coordination refusal without embedding an external key, password or payload.

| Phase | Meaning |
| --- | --- |
| BeforeOperation | Input validation and original native preflight have succeeded; no write has started. Later input/CAS/graph validation can still refuse. |
| BeforeBlob | Publish only: immediately before the existing object publication path. No new object has been selected. |
| BeforeCommit | Immediately before the actual metadata commit. Publish has already durably selected its blob; metadata edits retain their original reader(s). |
| Committed | The original transaction has committed and the native guard has accepted its own exact WAL advancement. |
| Verified | Existing native graph, metadata and relevant payload verification have completed, before final native result verification. |

Each actual write boundary first executes the original native selection check,
then the external callback, then another native selection check if the callback
returns success. A successful callback cannot hide a namespace change that it
observed or introduced. The final existing native state proof still runs.

Publish retains its selected new blob through BeforeCommit, Committed and Verified.
Rename/remove retain the selected existing payload. A real quota update retains
all physical payload readers, including orphans. Existing metadata/object writer
owners remain held. The native common directory is retained; this change adds no
directory flock. AccountRoot's separate common-root ownership is not implied here.

## Refusal, uncertainty and unwind

BeforeOperation or BeforeBlob rejection on unchanged sources leaves a healthy
native owner and no new object/metadata. Once a publish has selected a durable
blob, rejection returns OutcomeUnknown and preserves that physical object. A
pre-commit orphan is charged but has no visible logical reference after recovery.

Metadata rename/remove/quota rejection at BeforeCommit has attempted no commit:
the WAL remains unchanged, and GuardRejected is returned. The existing conservative
staged-operation rule still poisons that native owner; reopen is required. After
commit, rejection returns OutcomeUnknown, preserves confirmed state and requires
reopen. No automatic retry, cleanup or simulated rollback is performed.

An unchanged name or quota is a no-op: only BeforeOperation and Verified run, with
no metadata commit. A simple Verified refusal on unchanged sources stays an ordinary
guard error and leaves the owner healthy. Native source changes remain failures.

A trusted callback can panic. The guarded entry points catch unwind only to poison
the partially observed owner and resume the original panic. They do not turn it
into successful output or silently recover. This is not a panic path for remotely
provided input. No unsafe code is added; mutable state is not reused after unwind.

The callback itself must enforce any outer authority and withhold outer results
until its enclosing operation succeeds. A phase label and copied metadata grant
no permission for a later operation. Checks do not exclude arbitrary privileged
host changes between observations or constitute physical power-loss proof.

## Verification

Eight new test cases cover phase ordering against actual WAL bytes; live writer
exclusion and original descriptor lifetimes; each refusal and unwind boundary;
no-op behavior; unchanged invalid/CAS inputs; source changes despite a successful
callback; independent capture/restore; and 32 generated recovered-state models.

An actual child-process matrix executes 54 signal9 kills per toolchain: both WAL
versions, publish with empty/8193-byte payload, rename, remove, quota, every reachable
guard phase and the point after the API actually returns. The parent verifies the
signal, unchanged pre-commit WAL, recovered revisions/references/quota, preserved
physical payload/orphan charge, and a subsequent independent guarded write.

The first preliminary owner test incorrectly expected a native directory flock.
It was corrected to assert the actual retained directory descriptor and both real
component writer owners; no implementation lock was added to satisfy the fixture.
Final source-bound results are recorded in the
[verification report](measurements/2026-10-19-native-write-guards/verification.json).
Persisted formats, parsers and dependencies are unchanged. Existing lower-level
corruption/torn-write/commit-sync checks remain separate evidence. Current account
authority integration, user file policy, HTTP/SDK writes and release gates remain open.
