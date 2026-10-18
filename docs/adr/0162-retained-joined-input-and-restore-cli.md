# ADR0162: confirm the retained archive inside the last locked restore proof

Status: accepted for experimental native Linux path restoration and operator CLI.
Explicit live v2 adoption and production acceptance remain open.

## Decision

Reuse the joined publisher/inspector's private native source selection/type/privacy/
length helpers. Retain the original source parent/file, bounded immutable encoded
bytes and exact EOF/content/stamp/name checks. Expose a private confirmation closure,
not a source descriptor or mutable store, to the common restore implementation.

Confirm input before staging and around the last common physical proofs while its
root lock/original readers remain held. Do not call a complete byte restore and only
then check source: that composition releases the common lock before source acceptance.
The in-memory API uses the same implementation with a no-op source confirmation.
Existing readonly inspection protocols and archive/WAL formats remain unchanged.

Add a path API and account-file-restore ARCHIVE ROOT --time TIME. Require explicit
trusted signed-domain time; emit only bounded counts and decimal time. Preserve
existing targets, changed sources, private stages and selected uncertainty. Output
failure after durable completion preserves the copy and exits unsuccessfully.

## Consequences

Original source path authority remains strict while the native destination parent
may move without adoption of a foreign former path. Readonly/writable private inputs
and non-UTF8 source leaves are supported; parent/file links and special/broad/oversize
inputs refuse safely. Source changes before selection withhold publication; changes
after selection withhold the common receipt as ComponentRestoreUnknown.

This is trusted native offline operation, not current user authority, authenticated
backup provenance, encryption, a namespace lease or live v2 startup. Existing v1
startup still refuses the restored layout. Numeric bounds are not global resource
reservations; acknowledged preparation is not promised to roll back.

Verify original source/lock lifetimes at the last proof, before/after-selection
source mutations, independent copied writes and original sessions, parent movement,
generated cases, dedicated actual kills and actual binary confidentiality/argument/
FIFO/size/link/output/large-time behavior. See the [contract](../account-file-restore-cli.md).
