# Selected object retention under the native owner

`ProjectDirectory::put_selected` publishes an immutable object through the existing
native path and returns SelectedObject. `put_bounded_selected` runs the existing
complete bounded write and returns SelectedWrite with its original inventory
receipt. Both keep the actual selected file descriptor open and borrow the original
ProjectDirectory owner. Private fields and the lifetime prevent constructing,
cloning, serializing or releasing that owner while later verification uses the
selection. No raw file handle or write operation is exposed.

Expected project/object/report metadata can be read for subsequent native caller
work. It is not current user authority, a catalog commit or a filesystem lease.
`verify()` rechecks the retained owner/marker, streams the complete actual selected
object under its expected scope/ID, compares the original length/hash, and checks
private/stable metadata and visible inode. Owner admission and file metadata/inode
are checked again before success. Same-byte replacement at the filename is refused
even when an independent inspection of that replacement has the same report.
Any failed revalidation is PublicationUnknown for an already-published result;
the handle never removes, repairs, overwrites or reopens a replacement name.

SelectedWrite's inventory is the complete expected receipt at its original bounded
publication. Later verify checks the exact object and scope only. It does not
refresh that inventory or certify the complete directory against a later unmanaged
sibling. Per-call limits are still not authoritative persisted storage quotas.

The later [native catalog increment](native-file-catalog.md) adds explicit
SelectedWrite::verify_complete for callers needing the original complete receipt
under the retained selected descriptor. It hashes full inventory twice and verifies
the actual selection before, between and after; expected inventory is not refreshed.
The old verify contract remains exact-object-only. A last inventory observation is
not a lease against a subsequent sibling change. The standalone catalog uses its
own persisted quotas; this lower-level guard still supplies no account authority.

Moving the native directory does not redirect its retained handle into a replacement
path. A visible namespace can still change after the last observation, and native
operator/ancestor trust remains necessary. These guards neither sandbox a hostile
administrator nor retain current account/session/admission/file-policy authority.

The original put/put_bounded return types, capacity admission, selected-file byte
checks, fsync and uncertain outcomes remain. Bounded execution factors its existing
final selected descriptor/receipt into a private helper; old entry points discard
the descriptor at the same result boundary, new entry points retain it. No stored
format, backup/restore semantics or CLI/HTTP route changes.

This is one native retention prerequisite for
[ADR0127](adr/0127-proposed-account-root-object-integration.md), not an accepted
cross-catalog transaction or implemented root file service. Current authority,
schema/version, ownership order, orphan accounting, persisted quotas, common backup
and restore still need their own integration and evidence before user endpoints.
See [ADR0132](adr/0132-selected-native-object-owner-retention.md).

## Executed checks

Stable1.99 and minimum1.89 each passed209 relevant checks:126 object cases, two
owner-lifetime compile-fail doctests,54 storage and27 CLI. Eight new regular cases
include64 generated native binary selections, original owner lock/descriptor
retention, equal-report/same-byte replacement, moved source and foreign replacement
path, expected complete receipt boundaries, empty/8 MiB and private readonly files.
Thirteen byte/scope/length/mode/link/inode/marker/directory mutations before explicit
verification and the same13 after its complete body pass all refuse without cleanup.

All582 frozen hashes match. Formatting, strict stable workspace/fuzz lint, builds
and minimum all-fuzz compilation exit0. Existing25 process-kill boundaries reran;
no new durable boundary or kill is claimed. No parser/format changes or new native
filesystem sanitizer result is asserted. See
[source-bound evidence](measurements/2026-10-10-selected-objects/verification.json).
The separate complete24444bc workspace run is still pending and cannot establish
a full result for these later handles; older1518-test evidence belongs to d988a51.
