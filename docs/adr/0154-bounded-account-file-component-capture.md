# ADR0154: bounded account/file capture through original component owners

Status: accepted for experimental synchronous native Linux capture only.

## Context

AccountRoot retains original public/private owners across internal capture work.
NativeFileRoot can retain every actual physical payload descriptor across later
work, including charged orphans. Independently returned component images cannot
prove that longer shared lifetime. The next step must compose those boundaries
without installing a half-supported v2 filesystem layout or user file API.

## Decision

Add `capture_account_file_components(&mut AccountRoot, &mut [NativeFileRoot])`.
The caller explicitly supplies already selected file roots, strictly sorted by
canonical project identity. Admit at most four roots and128 physical objects in
total, including orphans, before account prefix capture/object reader retention.
Require every file project to belong to the existing declared private roster.
Never infer missing stores, sort invalid input, adopt filenames or create stores.

Retain the existing AccountRoot capture boundary and recursively nest original
NativeFileRoot callbacks with a fixed, at-most-four-level helper. Before file
readers, check database IDs globally across public, private and file components.
Compare copied file project/database/revision/quota/counts against preflight
observations. Flatten both native callback result layers. Final original source
checks must all succeed before the immutable common value can escape.

Produce an opaque AccountFileSnapshot containing the existing account snapshot,
native file snapshots, canonical copied v2 declaration and exact byte/object
charges. Preserve the source reset timestamp. Return no source descriptors,
serialization of secrets, current permissions or mutable engine handle.

Cap retained encoded image bytes at128MiB: account bundle, encoded declaration,
file metadata backup images and every native object's96-byte header plus payload.
Check charges after each bounded component copy and before retaining the next.
The cap does not include decoded models, vector capacity, descriptors, native copy
scratch or whole-process heap. The native single-component bound still governs
temporary copying; this is explicitly not preallocated memory/worker admission.

## Consequences

The v1 account filesystem layout and old file/archive formats remain unchanged.
The copied v2 declaration describes these explicit selected components; ordinary
AccountRoot continues to refuse v2 startup. No common path is promised. Joined
archive encoding, no-replace publication, original-owner restore and operational
adoption require separate increments. Current user file policy/upload/download
and deployment remain open.

Read-only capture neither resets sessions nor advances clocks. A failed aggregate
budget/caller check is not source corruption and leaves healthy file roots usable.
A changed native source poisons that file root and blocks the whole result. No
failed capture repairs, adopts or deletes source/foreign files. Checks are finite
observations under cooperating native owners, not a sandbox against the OS owner.

Validation covers actual nested locks/physical descriptors, orphan charging,
strict admission, byte boundaries, source substitutions, source immutability,
component restore independence, generated mixed models and actual signal9 kills.
The source-bound report distinguishes local affected suites from full hosted CI.
