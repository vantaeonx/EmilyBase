# ADR 0035: owned authoritative journal replacement

Status: accepted for the experimental synchronous Linux compaction API.

## Context

The stable directory lock excluded cooperating owners across WAL rename, but
compaction still followed `Database.path` for removal, creation and replacement.
Three independent failing regressions reproduced foreign-directory overwrite,
substituted staging selection/cleanup and acceptance of a detached selected
baseline. A fourth reproduced same-inode content rewriting after validation.
These paths differ from raw page creation and offline backup publication because
compaction deliberately replaces the one mandatory authoritative journal.

## Decision

Use the database's existing owned directory descriptor for the reserved staging
entry and selected `redo.wal`. Before starting or replacing anything, open the
selected entry without following links or blocking on nonregular objects and
compare its inode with the live WAL handle. A changed authoritative selection
poisons the owner: it must not acknowledge later writes to a detached journal.

An explicit maintenance request may remove the initial reserved `redo-next.wal`
entry inside that owned directory. Create its new 0600 regular single-link file
through the same directory and retain an inode-specific cleanup guard. Add
`Wal::create_snapshot_from_file` to initialize an empty owned file without path
resolution or truncating an existing file. Invalid parameters/admission and
competing ownership are refused. The original pathname API remains available;
the caller of the descriptor API is responsible for namespace publication/sync.

After sync and the private test boundary, validate staging identity/admission,
the old selected WAL identity, recovered baseline identity/transaction and exact
relational page images. Rename through the owned directory, mark publication
before exposing the post-rename boundary, then sync that exact directory. Reread
the selected owned image in fixed-size chunks and compare with the validated
bytes, checking identity/admission/length again. This reread does not move the WAL
file position or allocate another complete image. Only then adopt the replacement
handle and report success. Both old/new handles remain alive through publication.

Before rename, an active guard cleans only its own unchanged staging entry;
detached or substituted entries remain untouched. After rename, sync, identity,
admission or content failure preserves the selected outcome, poisons the owner
and reports `MaintenanceUnknown`. No automatic retry, staging adoption or cache
fallback repairs an uncertain authoritative selection. A directory rename itself
does not redirect this descriptor-bound operation or prevent valid compaction.

## Compatibility and limits

WAL 1/2, page/table bytes, transaction IDs and compaction report fields are unchanged.
Compaction still explicitly converts the selected journal to a self-contained
version-2 baseline; new databases still begin with version 1. It retains history
and is not vacuuming. The 64-MiB journal and normal transaction bounds remain.
Baseline encoding/recovery retains their existing bounded allocations; this is
not a whole-process memory bound. Directory entry names remain reserved.

The initial managed-directory pathname and operator-selected ancestors retain
their existing trust boundary. Observed identity/content checks are not a hostile
local-administrator sandbox or proof of physical power-loss behavior. Broader
failing-media, random-crash, upgrade and security/production gates remain open.

## Verification scope

The suite includes old failing regressions, before/after-rename truncation, CRC
damage and valid foreign history, source selection changes, staging links/modes,
twelve external native substitutions, the existing eight kill boundaries and
four directory-sync failures, plus a 32-case independent live-row model with
repeated directory moves, checkpoints, reopen and later independent writes.
Owned-file WAL tests check exact descriptor selection, byte preservation,
invalid inputs/admission, continued transactions and lock lifetime across clones.
