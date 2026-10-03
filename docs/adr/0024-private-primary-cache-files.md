# ADR 0024: explicit private primary-cache publication

Status: accepted for experimental optional cache files.

## Context

EBTI images verify managed scope and row liveness, but previously had no file
publisher. A cache must not change relational acknowledgment or redirect a write
through an exchanged directory. Partially written or old caches must never be
mistaken for current table data.

## Decision

Add synchronous save/load methods under existing exclusive database ownership.
Derive the only active filename from the validated nonzero table ID:
primary-ID.table-index. Require a private 0700 final directory matching the owned
directory inode/device, and private regular 0600 single-link files. Open files
relative to that owner with NOFOLLOW/NONBLOCK and validate metadata after open.
No arbitrary caller-supplied cache filename enters the managed directory.

Stage a randomized private exclusive file through the directory descriptor;
write, fsync and reread exact bytes. Check directory, staging identity and previous
active image before descriptor-relative publication. Initial creation uses
NOREPLACE. Replacement requires a structurally valid existing image bound to the
same database/table; it may be stale. Damaged, foreign or unsafe paths are preserved
and refused. Fsync the owned directory before reporting cache success.

Explicit load returns None for absence; otherwise it applies the full managed
image verification before installing the derived cell. Default open still uses
WAL recovery and reconstructs caches. Test-only hooks exercise sync errors and
kill boundaries; release builds contain no environment-controlled hooks.

## Consequences

Cache errors never poison valid relational state. After rename, a directory-sync
failure reports a distinct cache-publication-unknown error; confirmed table
transactions remain independently usable. Inspect/reload the optional file before
retrying cache maintenance. Private owner access is trusted; arbitrary privileged
filesystem attackers and physical power loss are outside this acceptance claim.

Descriptor-relative staging/publication/cleanup cannot follow a swapped root into
another directory. Cleanup removes only the still-matching own staging inode.
SIGKILL can leave staging files; they are never adopted or automatically deleted.
Dropped table IDs are retired, so their former cache is never loaded for a new
same-name table. Manual trusted housekeeping of orphan/obsolete caches is still
required. Files contain user keys and stay out of source control and backups.

CLI save/load commands use this exact library protocol. Existing formats and WAL
commit rules remain unchanged. Automatic adoption, transaction-driven refresh,
independently durable index roots/pages and secondary DDL remain future work.
