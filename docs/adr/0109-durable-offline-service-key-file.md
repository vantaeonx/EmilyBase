# ADR0109: durable offline service-key publication before activation

Status: accepted for trusted offline key-file publication after these checks.

## Context

A new private root intentionally returns no reusable key. Trusted filesystem
operators need a fully offline path from project metadata to a usable service key
without printing secrets, depending on an online master key or manually handling
an HTTP response. Existing rotation activates a digest in original project metadata;
key-file creation must not turn a confirmed rotation into an inaccessible secret.

## Decision

Add native AccountRoot.rotate_project_key_to_file and Rust CLI account-key-rotate.
Expose offline account-root-projects metadata to identify the target without
starting a server. These actions require the existing trusted filesystem owner,
not a user token or current key: this is the same operator authority as native
root initialization/rotation, never an HTTP privilege bypass. An unknown project,
existing target, busy/missing root or overflowing epoch refuses before activation.

Generate a fresh OS-random key in a zeroized buffer. Use the original identity-bound
Pending file owner outside the entire selected root. Resolve the retained parent,
so an ancestor alias into the root cannot add a secret to its strict inventory.
Write64 lowercase hex bytes to a new0600 singly linked file, fsync its descriptor,
publish with NOREPLACE, fsync/check its parent and verify the exact shared private
key-file contract. Only then activate the corresponding digest through original
registry metadata rotation. Retain both owners through final identity/content/
permission checks. Native/CLI receipts contain project identity and exact epoch
metadata, never the secret. Key files remain plaintext external configuration.

Factor existing rotation into a crate-private prepared-key primitive. External
callers still cannot choose a weak/arbitrary key. The existing public random-key
rotation/HTTP response contract remains available; its temporary source buffer now
zeroizes on error/drop. Original metadata fsync/poisoning behavior is retained.

## Consequences

This is an ordered two-publication workflow, not an atomic transaction spanning
registry metadata and an arbitrary external filesystem. A crash after file
publication but before activation can leave a valid private inactive key file.
The original key stays current until metadata publication. Any activated digest
has its new secret durably published first. Errors after publication require
inspection; metadata publication can already have changed the active key.

Never overwrite/reuse an uncertain existing file or automatically activate/retry
it. Reopen/inspect project epoch and use the file through a readonly current-key
operation; if necessary perform an explicit new rotation to a new filename.
Key rotation preserves users/session incarnation and both public/private WALs;
old service credentials refuse, current user sessions work with the new service
key. Verified root clone preserves the active digest but resets user sessions.
The external secret file is excluded from the bundle and needs separate protection.

A malicious operator/filesystem can remove a secret after any successful check.
Observed target/parent/content/permission substitutions refuse; foreign entries
are never overwritten/deleted. Failed preparation cleans only its retained owned
inode. Unpublished crash stages can remain private for operator inspection.
Lost or failing stdout may follow a complete activation: write errors propagate
without panic or a rollback promise. The service must be stopped for offline use.
No new stored format, online endpoint, role or production milestone is introduced.

## Verification

Seven new native cases cover both WALs, retained owners, protected/internal/alias
targets, five injected sync failures, parent/target/permission/content substitutions
before and after activation, and unchanged rows/private/sibling histories. Eight
forced-kill points on WAL1/2 include caller-received result:16 new kills per toolchain.
Five real CLI cases cover offline bootstrap/project metadata/user provisioning,
no-clobber/missing/busy roots, successive keys/session preservation/verified clone,
and actual output-write failure through the Linux full device. Existing rotation
kill/poisoning tests and all private HTTP guards rerun with complete CLI checks.

Final frozen-source checks pass135 cases on each Rust1.99/1.89: all87 CLI cases
across25 binaries, seven new native key-file cases, two original rotation cases
and39 private HTTP cases. Twelve regular cases are new. Strict workspace/fuzz
formatting/Clippy, minimum workspace build and all-target fuzz compilation pass
on487 source/dependency/protocol hashes. See
[source-bound evidence](../measurements/2026-10-09-offline-service-key-file/verification.json). Process kills are not machine power-loss
proof. No new parser/codec is introduced and no fresh sanitizer campaign is claimed.
Resource/load/upgrade/independent security/public admission/roles/production gates
remain open.
