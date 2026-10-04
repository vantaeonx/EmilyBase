# ADR 0034: owned page-file publication and checkpoint directories

Status: accepted for the experimental Linux storage and managed checkpoint APIs.

## Context

A raw page file is independent of the WAL transaction manager. A failing
regression reproduced its old pathname publisher accepting a replacement parent
and selecting a foreign file while returning a handle to the original inode.
A separate checkpoint regression reproduced a moved database directory directing
cache removal and replacement into the directory that occupied its old name.
Complete page validation does not establish namespace ownership.

## Decision

Raw creation pins a no-follow parent directory, anchors relative paths once and
creates a random private 0600 file through that handle. Validate sequential initial
page IDs before any file exists. Write and sync the complete header and pages,
then reread exact source bytes through the owned file descriptor with one bounded
page buffer. Verify parent and staging identity, regular single-link admission
and private mode before descriptor-relative no-replace rename.

Duplicate the file descriptor before publication, so duplication failure cannot
leave an ambiguous selection. Retain the exclusive lock through the returned
pager. Sync the original parent and check selected identity/admission before
success. Post-rename errors return `PublicationUnknown` and preserve the selected
file. Cleanup removes only a still-owned staging name; foreign entries and
detached originals remain untouched. Raw open refuses final symlinks, hard-link
aliases and nonregular files without blocking on a FIFO.

Add an explicit `Pager::create_with_pages_at` API for an already owned directory
and a single filename. Its contract intentionally addresses that exact directory
inode even when its name changes. Managed checkpoint removal, creation, rename
and directory sync now use the database's existing directory ownership handle.
Restore uses that API rather than requiring a symbolic `/proc/self/fd` parent to
pass the pathname API's no-follow policy. WAL remains authoritative; checkpoints
are disposable and do not reduce or replace committed history.

## Compatibility and limits

EMILYDB-1, EBPG-1, ETBL-1, both WAL versions and backup bytes remain unchanged.
Raw existing regular single-link files do not gain a new permission requirement;
new publication requires private mode. Alias rejection is an intentional admission
change. In-place raw writes remain nontransactional and cannot repair torn writes.
Linux local no-replace rename and directory sync are required. Final parent aliases
are refused by the pathname API; operator-selected ancestors remain trusted.
Observed identity checks are not a hostile local-administrator sandbox.

## Executed evidence

Old parent, checkpoint and staging-admission regressions fail before the fixes.
Cases cover substitution before/after rename, detached staging, aliases, readback
truncation/corruption/valid foreign bytes, eight sync failures before/after real
fsync, exact directory-handle selection and an independent 32-case page model.
Three native process kills cover synced staging, rename and a returned pager;
selected complete files reopen and accept independent writes. Checkpoint tests
cover moved/absent or substituted directories with WAL 1/2 and preserve foreign
files. Actual CLI checks cover relative Unicode paths, private modes, no clobber
and alias denial without printing record contents. Hardware power loss, broader
failing-media campaigns, stable upgrades and production/security gates remain open.
