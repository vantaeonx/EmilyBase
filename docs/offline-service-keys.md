# Offline project service-key files

A trusted local operator can obtain the first usable project service key in an
existing stopped private root without starting HTTP or printing the secret.
The same commands deliberately rotate later service keys. Use synthetic data;
this is privileged filesystem maintenance, not a user-token or remote API action.

```sh
emilybase account-root-projects private-root
emilybase account-key-rotate private-root PROJECT_ID --output service.key
emilybase account-user private-root PROJECT_ID --key-file service.key list
```

The first command returns `{"projects":[{"id":...,"name":...,"key_epoch":"1"}]}`.
Use the exact32-character lowercase hexadecimal project ID from trusted metadata;
display names are not IDs. Only metadata is printed. The root must exist and be
exclusively owned; opening never creates or resets it. No current service/master
credential is required because the local operator already controls the private
filesystem owner. This authority is never exposed to anonymous/network clients.

Rotation creates a new file containing exactly64 lowercase hexadecimal key bytes,
without a newline. Its exact mode is0600 and it has one hard link. The target must
not exist, including dangling links/directories, and must be outside the entire
selected root. The retained parent is checked so aliases into the root refuse.
Trusted operators control ancestors and protect the plaintext secret file.

The original owned staging mechanism writes/fsyncs the secret, publishes with
NOREPLACE and fsyncs/checks its parent before activating the matching digest in
original project metadata. Final identity/content/permission checks precede success.
The receipt is `{"project":...,"key_epoch":"2"}`, with an exact decimal-string
u64 epoch. No key, password, session, verifier or row content appears in stdout.
Metadata output is bounded to65,536 bytes and write errors return failure without
panic. Keep service files out of repositories and apart from root bundles.

There is no atomic transaction across metadata and an arbitrary external file.
A failure/crash can leave a complete but inactive key file. The old service key
remains active until metadata publication; every activated new digest had its
secret published durably first. A later error or lost stdout can also follow a
complete activation. Do not infer rollback from a missing receipt.

Inspect current project metadata after reopening and use the file with a readonly
current-key operation, such as account-user list. An unconfirmed file must not be
overwritten or automatically activated/retried. Deliberate recovery can rotate to
a new filename; this creates a fresh key and a new epoch. Protect/remove obsolete
files separately only after confirming the intended active key. The command never
performs automatic cleanup of foreign files or an operator's uncertain target.

Existing targets are never replaced. A failed preparation removes only its retained
original stage inode; a substituted foreign stage/name is preserved. Unpublished
stages left by a killed process remain private for operator inspection. Observed
parent/target/content/permission changes before activation keep the original digest;
a change observed afterward returns uncertainty rather than a false success.
Host/filesystem administrators can remove or alter a secret after any check; the
workflow does not sandbox that operator or prove durability under faulty hardware.

Rotation preserves public/private WAL bytes, users, policy definitions, passwords
and current user-session incarnation. Old service keys refuse subsequent operations.
A still-current user token remains valid when presented with the new service key;
rotation is separate from user disable/password/session revocation.
Verified root backup/restore preserves the active digest and revokes old user tokens.
The external key file is not copied into the bundle; protect/back it up separately
and deliberately provide it to the restored service/operator workflow.

Native trusted code can call AccountRoot.rotate_project_key_to_file. The existing
online administrative rotation remains available and continues returning its
protected response; the offline path adds no network endpoint. See
[private file loading](master-key-files.md), [offline users](user-cli.md),
[offline policies](policy-cli.md) and [ADR0109](adr/0109-durable-offline-service-key-file.md).
Public user admission, roles, encryption of secrets and broader load/security/
upgrade/resource/production gates remain open.
