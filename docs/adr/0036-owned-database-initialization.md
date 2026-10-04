# ADR 0036: owned database initialization and journal admission

Status: accepted for the experimental synchronous Linux managed database API.

## Context

Three independently failing regressions reproduced creation/opening through a
replacement directory while holding the original directory lock, or returning a
database after its named directory changed during initialization/recovery. A
separate raw WAL regression reproduced a final symbolic alias being followed.
The compaction directory binding does not establish initial file ownership.

## Decision

Anchor relative managed paths once. Creation pins a no-follow parent directory,
creates the new private directory through that handle, opens/locks the exact
directory and verifies parent/name identities before initializing anything.
Open existing managed directories with no-follow directory admission. Before and
after journal recovery, verify the named directory matches the locked handle.

Create/open `redo.wal` through that directory, without following links or blocking
on nonregular inputs. `Wal::create_from_file` initializes a private empty file with
the original version-1 header; `Wal::open_from_file` validates a regular single-link
file and seeks to offset zero before bounded read/recovery. The raw pathname WAL
open now applies the same final no-follow/nonblocking admission. Existing regular
file modes remain compatible on open; new files require 0600. The caller supplies
an exclusive owned open-file description and remains responsible for publication;
clones retain its lock and must not be used as additional concurrent writers.

After initial root commit, sync the owned data directory and captured parent,
check both named identities and selected WAL inode again, and verify newly created
0700/0600 admission. A failure after initial commit returns `InitializationUnknown`
and preserves the result for inspection. Earlier namespace refusal writes nothing
into a replacement directory. Opening also confirms the selected leaf matches
the exact recovered WAL handle. Every refused constructor releases its handles.

## Durability and compatibility

Managed directory creation remains detectably interruptible: it does not publish
through an atomic directory rename. An existing partial directory cannot be
silently reinitialized or replaced. A missing/uncommitted initial root fails
recovery; a complete unobserved internal initialization commit may be inspected
after a constructor failure. Neither staging nor a checkpoint authorizes missing
mandatory WAL recovery. This differs from the raw page-file publisher.

WAL 1/2, EMILYDB/EBPG/ETBL, backups and HTTP/SDK shapes are unchanged. Relative
paths are retained as absolute operator paths internally. Final directory/WAL
aliases and multiple hard links are deliberately refused; ancestor directories
remain operator-trusted. Linux `/proc` descriptor paths ending in `/.` still
address actual directory inodes for verified restore. The existing safe filesystem
library gains one WAL dependency edge; no package versions or storage engine change.

Observed boundary checks are not a hostile local-administrator sandbox. Physical
power loss, broader failing-media/crash campaigns, stable upgrades and completed
security/production acceptance remain open.

## Verification scope

The suite covers the old failures, creation/recovery parent and leaf substitution,
directory/file mode and link changes, four before/after real sync failures, three
native creation kills and eight external native directory changes. A 32-case
independent generated-row model uses both WAL versions, refuses mismatched named
ownership, preserves both histories and permits independent later writes.
Actual CLI checks cover relative Unicode paths, private creation, continued
checkpoint/compaction/reopen and alias refusal without disclosing rows/paths.
Owned WAL tests verify both versions from nonzero descriptor positions, exact
original header bytes, type/size/identity refusal and input byte preservation.
