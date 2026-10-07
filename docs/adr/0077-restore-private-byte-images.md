# ADR0077: direct byte-image restore through the owned preparation boundary

Status: accepted for single-store byte restore; combined root restore open.

## Decision

A combined bundle already owns verified nested archive bytes. Requiring each
nested private image to be written as another sensitive input archive adds files
and cleanup obligations without improving validation. Add restore_bytes and
restore_prepared_bytes to the original backup library, and
restore_private_account_bytes to the separate scoped account library.

File restore still performs its existing bounded private regular-file read. Both
inputs then use one bounded byte-image staging/replay/preparation/publication
implementation. Input bytes are inspected completely before creating a target
stage or invoking application preparation. The original acknowledged WAL is
written to a private descriptor-owned stage, synced and reopened against its
expected database identity. Preparation runs privately and must release owners.
Final reopen/replay, checkpoint, directory sync and atomic no-replace publication
retain existing identity checks and uncertain post-rename error handling.
Returned counts describe the installed prepared state, not the source image.

Ordinary restore_bytes does not change application semantics: it preserves the
historical private session incarnation just like generic file restore. The
private byte wrapper is mandatory for account stores: validate independently
expected project/time and complete private schemas, reset v3 to a fresh durable
scope/time or explicitly activate v1/v2, then publish. Existing file and new byte
private entry points share exactly that preparation and typed error mapping.
Old access/refresh tokens remain denied from the first published private state.
Reset/activation needs WAL headroom, as with private file restore. A capped journal
fails before selection; restoration does not silently compact the private archive.
Passwords and account metadata remain the restored historical data; source state
and source bytes are never changed. No token is issued automatically by restore.

Byte inputs have the same bounded experimental envelope/WAL parser. They do not
carry filesystem source permissions, authenticity, provenance or request authority.
The caller supplies sensitive bytes and trusted destination/time/project. No input
archive file is created by the byte API; output WAL/checkpoint/staging files still
exist and are synced. Per-format bounds are not a whole-process heap reservation.
No database engine, runtime dependency, implicit wire-version conversion or
network/CLI restore route is added.

## Evidence and compatibility

Three new backup cases cover both WAL versions, direct and prepared installed
reports, source-byte/history immutability, input lifetime, absence of intermediate
archive files, malformed bytes before preparation/staging, typed callback refusal,
owned cleanup, no replacement and foreign directory/symlink preservation.

The existing private version/WAL matrix now executes both file and byte inputs:
12 combinations preserve credentials while resetting scope before publication.
The32-case independent disabled/epoch/clock model now restores through the byte
wrapper. A new private case rejects invalid scope/time, wrong project and a fully
engine-valid but private-invalid zero epoch without publication. Existing private
schema, owner and file-restore checks remain in the regression suite.

Existing native helpers add four ordinary byte-restore process kills at synced/
published boundaries and four private byte-restore kills at reset-prepared/returned
boundaries across WAL1/2. Selected states are complete, old private tokens remain
denied, source histories stay exact and retries use verified separate publication.
No new helper is silently skipped, and no new parser ASan or physical power-loss
campaign is claimed. The unchanged byte grammar is covered by prior parser runs;
actual full final checks are recorded in testing.md.

## Next gate

The byte API removes an input-file requirement for nested restoration; it does not
publish a combined platform root. A coordinator must own the target root, restore
all selected registry/private entries, validate their complete roster and reset
every private scope before the root can be selected or serve traffic. Authoritative
private attachment metadata, HTTP users/workers, roles/RLS, numeric resource
reservations and production security/upgrade/load/crash acceptance remain open.
