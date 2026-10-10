# Native persisted quota administration

`FileStore::quota_state()` returns the current operator FileQuota, project,
metadata database identity and global metadata transaction revision after complete
graph verification. This opaque copied QuotaState is scoped CAS metadata, not user
authority, a reservation or proof of unique ancestry across identical restored copies.
The existing quota getter remains a value-only inspection.

`set_quota(expected_state, new_quota)` requires the exact current project/database,
global revision and stored quota. A state from another database refuses even when
its project/revision/value match. Any later reference or quota commit makes an older
state stale. Current equal limits are a no-op preserving WAL bytes; stale equal
limits still conflict. This deliberately conservative global CAS requires no new
stored revision field or private schema migration.

Before staging, every existing physical object, including unreferenced orphans,
must fit the proposed object/payload limits. Logical removal does not make its
physical bytes free. A zero object limit can close an empty store. FileQuota keeps
the existing128-object/64 MiB payload bounds and8 MiB individual object bound.
Headers, metadata history, operating-system caches and total memory remain outside
this payload quota. There is no per-request untrusted override.

For an actual change, the operation opens and fully hashes each physical object's
actual readonly descriptor against the complete inventory, including every orphan.
At most128 descriptors remain retained under the original directory owner through
staging, revalidation, the original-WAL scope-row commit and final source/graph/
inventory checks. All expected file references remain unchanged. Partial admission
failure drops acquired descriptors before staging and changes neither resource.

Detected source failure after staging and before a commit attempt discards staging,
returns a direct error and requires FileStore reopen. After a commit attempt, errors
are OutcomeUnknown and poison FileStore. No source file is removed/repaired/replaced
and no uncertain request is retried. The commit changes only the existing scope
row's limit values. Version1 schemas, original formats, file fsync and WAL ACK remain.

Holding these bounded handles is native resource usage, not a global server FD or
worker reservation. The operation performs repeated full hashing and makes no
throughput claim. Native operator/ancestor trust remains; later namespace changes
are outside a final observation's lease. This library is still outside current
server/CLI/AccountRoot, user policies and coordinated backup/restore. See
[ADR0137](adr/0137-scoped-native-quota-cas-and-retained-inventory.md).

## Executed checks

See [source-bound verification](measurements/2026-10-10-file-quota/verification.json).
Coverage includes native WAL1/2, exact current no-op versus stale equal state,
cross-database identity, reference commits invalidating global quota CAS, orphan
minimum charge and independent reopen. Eight identical-byte inode replacements
cover referenced/orphan objects before/after commit on both WAL versions.

The Linux maximum-inventory check measures128 additional retained descriptors at
staging and returns to the exact prior FD count after success. The generated model
tracks physical charge, visibility, limits and global revisions through mixed
quota/publish/rename/remove histories with reopen after every step. Actual process
kills cover staged, committed-before-response and caller-success quota changes for
both WALs and empty/8193-byte objects, followed by independent recovered writes.

The final affected matrix passed570 checks on each Rust1.99/1.89 toolchain:
249 native files/storage/CLI and321 database/WAL/transactions/backup. All596
source hashes matched before/after. Formatting, strict workspace/fuzz lint,
explicit builds and minimum-toolchain fuzz builds passed. The12 new quota kills
and55 previous native kills passed on each toolchain. No new sanitizer campaign
or complete current-workspace result is implied. The preceding record codec,
fuzz decoder and both lockfiles remain byte-identical.
