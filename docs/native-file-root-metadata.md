# Guarded native common-root metadata mutations

`NativeFileRoot` supports copied `file_info`/`quota_state` observations, reference
rename/removal and persisted physical quota updates. All are native Linux operator
operations on the retained original common root. Owner fields remain catalog data;
these methods do not authenticate an account or expose a user-file HTTP service.

Rename/removal require the exact logical reference revision. Quota updates require
the opaque project/database/global-revision state returned by `quota_state`.
An unrelated reference commit invalidates a previous quota observation. An equal
current name/quota is a verified no-op with unchanged WAL; a stale equal request
still conflicts. Missing removals are Missing, not assumed successful retries.

## Original commit and selection

The existing synchronous FileStore mutation/quota transactions remain the only
writers. Internal fallible hooks guard the retained parent/root/children, original
scope and unchanged original WAL before commit. After the actual engine ACK the
same retained readonly WAL inode must byte-match the original database's verified
acknowledged history, with private identity and a stable stamp around readback.
Only that original inode's baseline advances. Final source and graph checks still
precede success. Standalone FileStore entry points retain their separate pair
contract; they do not silently acquire common-root selection authority.

Rename/removal retain the existing referenced ObjectReader across their transaction.
Quota changes retain every original physical ObjectReader, including orphans, up to
the existing128-object bound. Common-root guards add namespace and WAL verification
around those original lifetimes. Final metadata-only validation requires identical
physical inventory, unchanged unrelated references and exact expected catalog/quota.

Removal changes logical visibility only. Bytes remain private, immutable, included
in coordinated capture and charged against object-count and byte limits. Lowering
quota below physical inventory fails even when no logical references remain.
There is no physical garbage collection or implicit quota release.

Invalid input, missing/stale reference, wrong quota scope or insufficient limits
preserve the usable owner when source remains unchanged. A detected source failure
poisons it. Staged failure before commit is direct and rolls back metadata. Once a
commit is attempted, a later failure is OutcomeUnknown; data and changed sources
are preserved for explicit inspection/reopen. No automatic repair, retry or cleanup.

## Native CLI

```sh
emilybase file-root-stat root 01010101010101010101010101010101 02020202020202020202020202020202
emilybase file-root-rename root 01010101010101010101010101010101 02020202020202020202020202020202 --revision 3 --name renamed
emilybase file-root-remove root 01010101010101010101010101010101 02020202020202020202020202020202 --revision 4
emilybase file-root-info root 01010101010101010101010101010101
emilybase file-root-quota root 01010101010101010101010101010101 --database EXACT_DATABASE_ID --revision 5 --max-objects 2 --max-bytes 9
```

Use the actual database ID/global revision from file-root-info and actual logical
revision from file-root-stat. The illustrative numbers depend on prior history.
Canonical identifiers and bounded quota values are parsed before owner admission.
The CLI compares explicit database/global revision with the original scoped state
before updating; it does not silently replace a stale observation with a fresh one.
All output contains only counts, identifiers, revisions and digests. Names, owners
and payloads are omitted. Failed stdout delivery after commit preserves the change;
stale retries cannot overwrite a later revision. An absent removal stays Missing.

## Verification and limits

[Source-bound evidence](measurements/2026-10-15-native-file-root-metadata/verification.json)
covers both WAL versions, exact no-ops/stale requests, foreign database CAS, physical
orphan charging, independent reopen and paired restore, and actual CLI processes.
The source substitution matrix spans7 root/child/WAL/scope/mode/roster changes,
3 metadata operations,3 transaction boundaries and2 WAL versions:126 cases.
Eighteen parent replacements and48 same-inode WAL rewrite/corruption/mode/foreign-tail
cases refuse without adopting or repairing the changed source. Thirty-two generated
histories compare an independent revision/visibility/physical-usage model.

Twenty-four actual SIGKILL cases span staged, acknowledged, verified and caller
success for rename/removal/quota on WAL1/2. Only committed changes recover; original
objects remain and the reopened root accepts a further independent update.
CLI cases cover exact metadata receipts, no-op WAL equality, malformed/foreign/stale
inputs, orphan quota and undelivered receipts. Native descriptor return is checked.

No file/schema/dependency version or original fsync/ACK rule changes. Bounded WAL
replay, graph validation and retained descriptors are not global resource leases or
network deadlines. Linux/proc, trusted ancestors and finite checks remain native
assumptions. AccountRoot integration, current user file policy/download/signed URLs,
physical power-loss evidence and production acceptance remain open. See
[ADR0147](adr/0147-guarded-common-root-reference-and-quota-mutations.md).
