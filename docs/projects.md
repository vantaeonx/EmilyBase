# Experimental isolated project registry

The synchronous `server` library provides a local private registry and one-shot
authorized SQL capabilities. The Axum server enforces separate master/project
credentials around privileged registry and scoped data methods; see [HTTP](server.md).
Privileged library callers can create/list projects and rotate their credentials.
User accounts, password hashing, roles, row policies and sessions are future work.

## Identity and authorization

IDs use 16 operating-system-random bytes encoded as exactly 32 lowercase hex digits.
Only such IDs select directories. Display names are 1..128 UTF-8 bytes, not blank,
without control characters; they never form paths. Names need not be unique.
Each project owns a separate managed `data/` database. SQL cannot select another
project's directory or attach outside files.

API keys use 32 random bytes encoded as 64 lowercase hex digits. Persist only
SHA-256 digests, compare fixed-size digests with `subtle::ConstantTimeEq`, and return
plaintext only to the privileged create/rotate caller. Debug representations redact
digests; errors never echo tokens. These keys are full project-owner credentials.
They are not user accounts or limited row-policy roles.

Successful rotation syncs and atomically replaces metadata before changing the
in-memory digest. The old key then fails new authorization; the new key survives
reopen. An already authorized request may finish after rotation. Capabilities are
consumed by execution and are not cloneable. They retain root ownership until
execution/drop, including when the registry controller itself is dropped.

## Layout and publication

Linux/local filesystem target. Root/project/data directories must exclude all
group/other permission bits; new directories use 0700. Metadata uses 0600. Root,
project, data and metadata symlinks are rejected. These guards assume a trusted
local owner; they do not sandbox a malicious administrator who can modify private
filesystem entries. The root inode has one exclusive advisory owner. Each project
has an independent request gate; same-project requests serialize before database
open. Database ownership/recovery/sync remains in the original engine.

```text
emilybase-data/
  <32-hex-project-id>/
    project.json
    data/
      redo.wal
      checkpoint.emily
  .creating-*/          # unacknowledged staging, never adopted automatically
```

Creation initializes/syncs a staged managed database and metadata, syncs the staged
directory, publishes with Linux no-replace rename, then syncs the root before ACK.
Rotation syncs a private temporary metadata file, atomically renames it and syncs
the project directory before ACK. Publication/sync uncertainty poisons the registry
until reopen. Incomplete creation staging stays outside project discovery and is
preserved for explicit operator inspection; there is no automatic cleanup/adoption.
No project deletion or format conversion exists.

## Metadata version 1

`project.json` is bounded to 4096 bytes, a strict JSON envelope with `payload` and
`checksum` fields. Payload order for canonical checksum bytes: `version`, `id`,
`name`, `key`, `epoch`. Version is 1; ID must match the directory; key is a 32-byte
digest array; epoch is a nonzero u64. Checksum is CRC32 of compact Serde JSON for
that typed payload. JSON whitespace is not significant. Unknown/duplicate fields,
unknown versions, wrong identities, malformed hashes and invalid names/epochs fail.
CRC detects accidental changes; it is not authentication or encryption.

At most 128 projects. Epoch overflow refuses rotation without changing bytes.
Root discovery fails closed on invalid committed entries. `inspect_project_metadata`
is a pure bounded decoder returning only public ID/name/epoch information.
Database/WAL/backup/index formats are unchanged. Registry metadata is experimental;
future incompatible changes require an explicit version/converter.

## Verification and open gates

Cross-key denial, scoped SQL, rotation/reopen, root ownership, 32 concurrent requests,
actual 128-project capacity, path traversal labels/IDs, symlinks and private modes
are tested. Truncation, repaired-CRC semantic violations, epoch overflow and ignored
staging execute. Generated rotation and metadata properties and bounded ASan parsing
execute. Actual registry publication kills/sync faults, HTTP master authorization,
worker/peer limits and local container deployment now execute. A bounded offline
[whole-registry backup](registry-backup-format.md) preserves current metadata and
committed histories, with verified no-clobber restore. Dedicated backup publication
kills/sync faults, full-capacity/model and restored HTTP/container checks execute.
Wider backup crash/fault,
physical-power-loss, load/security and production acceptance remain pending.
Use synthetic data only; this is not a production isolation guarantee.
