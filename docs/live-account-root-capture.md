# Native capture from a retained account root

`AccountRoot::capture(&mut self)` returns an immutable `AccountRootSnapshot` of
the existing v1 registry/private bundle and copied root metadata. The source root
remains open. No temporary close/reopen of the registry or private account stores
is needed. Ordinary and parent-relative account roots use the same implementation.

The snapshot exposes `bundle_bytes`, consuming `into_bytes`, copied `manifest`
and count-oriented `report`. Debug output contains only byte/private-store counts.
The bytes remain sensitive: they contain private credential/session material and
project service-key metadata. They must stay in operator-controlled private storage.
Snapshot ownership grants no user permission or access to a live source descriptor.

## Capture boundary

The original root, registry, private container and private-store owners remain
held throughout capture. The existing registry encoder acquires every original
public data owner before the first export and retains all of them until checked
coordination finishes. Private owners are the original stores already inside the
root; no detached account handle or registry capability escapes.

The internal coordination callback runs while all these owners remain held. Its
result is withheld on failed final checks. This is the boundary where future
file-component snapshots can run; it does not yet capture files, publish a joined
archive, restore a common root or provide a live HTTP backup endpoint.

Checks surround copying/coordination and include the original selected root,
declared inventory, private directories, manifest bytes/inode, original registry,
original public/private database directories and selected WAL inodes. WAL entries
must remain regular private single-link files. Parent-relative capture also checks
its retained private parent, while preserving the established moved-parent behavior.

Each copied acknowledged public WAL prefix and private backup image is compared
by SHA-256 with a fresh export from its original owner before release. Matching
final rows alone cannot accept a different acknowledged history. Byte equality
alone cannot accept a replacement inode. This uses the existing engine's validated
committed-prefix export; it does not invent another transaction or recovery engine.

Unacknowledged physical tails are outside the copied history. A tail introduced
under the already-held owners is excluded and left untouched by capture. This is
not a promise that the complete physical WAL file or its timestamps stay frozen.
Existing database opening/recovery behavior remains separate. No capture commit,
private clock advance, session reset, migration or checkpoint is performed.

Returned copied bytes hold no source descriptors, so they survive source release
and later legitimate source writes independently. Existing offline v1 restore
accepts them and applies its explicit session reset only to the restored copy.
The source's sessions remain valid under the original current admission rules.

## Bounds and limits

The active private-store limit remains4; the existing registry/bundle limits and
format are unchanged. A mutable root borrow serializes native capture with root
operations. Public database locks are acquired in the established registry order;
busy admission fails rather than exporting a partial bundle. Final prefix checking
adds bounded re-export/scratch work. Count/encoded-byte limits are not a reservation
of total heap, CPU or file descriptors, and retaining many returned snapshots has
additional caller-controlled cost.

Native callers and ancestor configuration remain trusted. File checks are finite
observations, not a filesystem lease or protection from a hostile administrator.
Callback errors do not publish output or repair source entries. The opaque copied
result is never current authority. File-capable v2 startup, common file ownership,
coordinated account/file publication/restore and current user file policies remain
open gates; this change does not complete that milestone.

See [ADR0151](adr/0151-live-capture-under-original-account-root-owners.md),
[retained roots](retained-account-root.md) and
[parent-relative selection](parent-relative-account-root.md).

## Executed evidence

[Source-bound checks](measurements/2026-10-15-account-root-capture/verification.json)
record796 affected Rust cases per stable1.99/minimum1.89, including the complete
server library/auth crate, registry integration, CLI and original engine suites.
SDK passes34 unit and12 actual local-server/restart tests.635 frozen source input
hashes match after all processes close; formatting, strict workspace/fuzz lint,
builds and minimum fuzz compilation succeed. No new sanitizer campaign is claimed.

Eleven snapshot cases cover exact offline v1 compatibility, original owner/lock
lifetimes, descriptor return, callback refusal, independent source/copy writes and
copy-only session revocation.16 generated live write/capture/fresh-restore models
compare independently selected rows. Tests reject same-byte WAL replacement,
corruption, nonprivate modes, hard links, another valid history with equal final
rows and17 selected namespace/manifest/parent changes. Four abandoned-tail cases
exclude and preserve physical tails. Twelve actual signal9 kills span dedicated
checked-capture owner, copied, coordinated and acknowledged boundaries for empty
and nonempty WAL1/2 roots; exact source history survives and credentials work again.
