# ADR 0032: owned backup publication destinations

Status: accepted for the experimental Linux single-database backup API.

## Context

Two executable regressions reproduced a publication error: after private staging
and verification, replacing the destination's parent and supplying a foreign
entry with the same temporary name could publish that entry and return the
original report. Path-based cleanup could also remove a substituted entry.
Source ownership and correct archive checksums do not bind an output pathname.

## Decision

Open the real destination parent once without following its final symlink and
anchor relative operator paths at admission. Keep its directory descriptor alive.
Create private random staging entries relative to that descriptor, hold their
file/directory handles, and bind entries by device/inode. Check the visible parent
and staging identity before publication. Reread staged archives through their
original file descriptor; use the owned restore directory through its Linux
`/proc/self/fd` handle when invoking the existing synchronous path-based engine.

Publish both archives and restored directories with descriptor-relative Linux
`renameat2(RENAME_NOREPLACE)`. Sync the owned parent and verify its visible
identity and selected entry before success. Refusal before rename leaves no
selected destination. A sync/identity failure after rename returns the existing
`PublicationUnknown` error and preserves the complete selection for inspection.
Never remove it or advise a blind overwrite retry.

Cleanup uses the pinned parent only when its staging entry still names the owned
inode. Detached originals and substituted entries are preserved. Process kills
may leave private staging artifacts; these are never adopted automatically.
Archive input uses a no-follow, nonblocking open and rejects nonregular files
before reading, avoiding FIFO/device input and bounding growth probes as before.

## Compatibility and limits

EMILYBAK-1 bytes, embedded WAL 1/2 bytes, database identity and transaction IDs
remain unchanged. Archives and directories remain 0600/0700. Public functions and
CLI report shapes are unchanged; final input/parent symlinks are now refused.
This publication API requires Linux, mounted `/proc`, local rename/fsync support
and trusted operator-selected ancestors. It is not a sandbox against a hostile
local owner/administrator or proof of actual hardware power-loss behavior.
Registry archives use a separate publisher and are outside this change.

## Executed evidence

The failing parent regressions pass after repair. Additional cases exercise
staging replacement, symlinks, post-publication parent changes, private modes,
nonregular input and exact old archive bytes. Twenty before/after-real-fsync
fault cases preserve source bytes and distinguish pre/post-publication outcomes.
Eight native subprocess substitutions and the existing eight publication kills
execute on both WAL versions. A 32-case independent row model verifies failed
publication, exact source preservation, verified retry/restore and independent
subsequent writes. Real CLI checks cover relative Unicode paths and alias refusal.
Wider failing-media, upgrade, random crash and security gates remain open.
