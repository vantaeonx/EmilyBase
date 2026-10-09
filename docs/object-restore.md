# Verified native object restore

`restore_archive` and `restore_archive_file` restore one complete experimental
EMILYOBK v1 image into a fresh private0700 directory. They preserve the expected
project, object IDs and exact canonical EMILYOBJ bytes. There is no rescoping,
overwrite, merge, live-project replacement or coordinated AccountRoot restore.
Use synthetic data only.

Verify the entire input, including scope/count/length/checksums/order/inventory
digest, before creating a stage. File input must be private, regular, singly
linked and bounded. It is retained and rechecked against its visible identity;
after reconstruction it is fully reread and compared before final selection.
Operator paths/ancestors remain trusted. These observations are not a filesystem
transaction or a lease against later native administrators' writes.

The original storage layer now provides `StagedPrivateDirectory`: a random
exclusive0700 directory in a retained destination parent, with exact stage
identity/private-mode checks. Populate it through a retained descriptor using
the original marker and immutable object publisher. Each child uses original
file/parent fsync and full byte readback. Verify the complete reconstructed
inventory before selection. Then fsync the complete directory, rename without
replacement, fsync its parent and retain both selected directory and parent.
Recheck final identity/parent and the complete owned inventory before returning
metadata. The cooperating directory lock spans reconstruction and publication.

Storage's generic directory helper grants native filesystem authority only and
requires its caller to fsync/verify children. It does not inspect database/object
formats or recursively clean files. Its returned selected identity supports
observations, not a permanent namespace lease.

Failures have distinct outcomes:

- A malformed/private-source refusal before staging creates no destination entry.
- `RestoreStage` means reconstruction/input recheck failed before selecting the
  final directory. An unchanged empty owned stage can be removed; a nonempty
  private stage remains for explicit operator inspection.
- A known no-replace collision preserves the existing destination and the complete
  unselected private stage. There is no recursive sweep or overwrite retry.
- `PublicationUnknown` means selection happened but final durability/identity/
  contents require inspection. The selected directory is preserved.
- Lost CLI stdout after a successful restore similarly leaves the selected data.

Do not automatically interpret any failure as permission to delete/retry an
existing target. Inspect complete state explicitly. Incomplete stages use random
`.emilybase-directory-` names; inspect the parent offline and decide cleanup
separately. This implementation does not supply an automatic stage janitor or a
durable recovery manifest for those names.

```sh
emilybase object-archive-restore ./copy.object-archive 01010101010101010101010101010101 ./restored-objects
emilybase object-directory ./restored-objects 01010101010101010101010101010101 list
```

CLI success emits the same bounded format/project/count/bytes/digest metadata as
archive verification/publication. Payloads are private files and never stdout.
Restored bytes are independent copies, not hard links to the archive or source.

Bounds remain128 objects,64 MiB aggregate payload,8 MiB per object and67,124,352
archive bytes. Native file reread can retain two complete archive images plus
bounded object buffers/metadata; this is not streaming or a global heap budget.
Checksums do not encrypt/authenticate data. Power-loss qualification, AccountRoot
coordination, HTTP/file policies, signed URLs, resource quotas and production
acceptance remain open.
