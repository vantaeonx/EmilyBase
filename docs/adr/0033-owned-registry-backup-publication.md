# ADR 0033: owned registry backup publication

Status: accepted for the experimental Linux offline registry API.

## Context

The single-database publisher and the registry publisher have independent staging
implementations. Two failing registry regressions reproduced parent substitution:
a foreign archive inode with matching bytes or an unrelated restore directory
could be published and acknowledged through a replacement parent. Path-based
temporary cleanup could also remove a foreign entry. Archive validation alone
does not bind a filesystem publication target.

## Decision

The registry publisher pins a no-follow parent directory and the exact staged
file/directory handles. It anchors relative operator paths once, verifies visible
parent and staged-entry device/inode identity before descriptor-relative
no-replace rename, syncs the owned parent and checks its selected entry before
success. Reread of the staged archive uses the owned regular private single-link
file descriptor. Directory restore writes through its live Linux `/proc/self/fd`
path, retaining explicit 0700 directory and 0600 file modes.

Disable generic temporary-path cleanup before exposing publication boundaries.
The owned guard removes only an unchanged entry in its pinned parent. Detached
originals and foreign entries are preserved. A failure after rename preserves
the selection and returns `PublicationUnknown`; it never overwrites or silently
retries an ambiguous destination. Mandatory WAL replay, complete registry-image
comparison and scoped-key/epoch checks still precede publication.

Keep this internal guard specific to the registry envelope. Its validation,
single-link admission, multiple project/data directory syncs and fault boundaries
differ from single-database backup. No public storage format or generic partially
implemented publication API is introduced. Common low-level operations use the
existing safe filesystem library; there is no original unsafe block.

## Compatibility and limits

EMILYREG-1, nested EMILYBAK-1 and WAL 1/2 bytes remain unchanged. IDs, active key
digests, rotation epochs, database identities and commit counters remain exact.
No plaintext project key or master key enters the archive. CLI/HTTP/SDK report
shapes are unchanged. Final parent symlinks are refused. Linux, mounted `/proc`,
local rename/fsync support and trusted operator-selected ancestors are required.
This guards observed namespace changes; it is not a hostile local-administrator
sandbox, cryptographic archive authentication or hardware power-loss proof.

## Executed evidence

Both old parent regressions first fail, then pass. Cases cover detached staging,
symlink entries, post-rename parent/selection changes and ancestor replacement
between restore WAL sync and later project writes. Eight independent native
subprocess substitutions and 64 refusal/uncertainty cycles verify exact source
preservation and descriptor release. Existing publication kills, fourteen sync
faults and competing restorers remain exercised. A 32-case independent model
covers empty/multiple projects, mixed WAL versions, generated values, key epochs,
scope denial, failed sync, verified restore and independent rotation/writes.
Actual CLI checks cover relative Unicode paths, empty/mixed registries and alias
refusal without disclosing IDs or credentials. Wider load, failing-media, upgrade,
security and production gates remain open.
