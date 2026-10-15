# Native account/file component capture

`emilybase_server::capture_account_file_components` takes an existing mutable
AccountRoot and a mutable slice of selected NativeFileRoot owners. It returns
sensitive immutable copies after all final source proofs succeed. The operation
is synchronous, trusted native code, outside every HTTP/user route.

## Scope and ownership

File roots must be strictly sorted by canonical project identity, unique, and
belong to the account root's explicit private roster. A subset, including empty,
is allowed; nothing is provisioned or inferred. Up to four file stores are admitted.
The account root's existing four-active-private-store limit still applies.

Preflight checks actual file metadata/graph/usage and copies root metadata. Then
AccountRoot holds every original public/private owner through the entire nested
file capture. Each original file root retains metadata/object ownership and every
actual payload descriptor, including physical orphans, until deeper work finishes
and that source passes final proofs. Globally repeated database identities across
public data, private accounts and file metadata are refused before file readers.

The immutable result exposes account/file snapshots, copied canonical manifest
bytes, manifest metadata and charged byte/object counts. Debug prints counts only.
It retains no native source descriptor and carries no current user authority.
Its file order equals the caller's admitted order and copied declaration order.
Sources, source reset time, sessions and clock floors are unchanged.

## Aggregate resource bounds

| Resource | Limit | Admission boundary |
| --- | --- | --- |
| File components | 4 | Before source preflight/capture |
| Physical objects including orphans | 128 total | Before account prefix/file readers |
| Retained encoded copied images | 128MiB | After each bounded native copy |

Byte charges include the account bundle, exact canonical v2 declaration bytes,
each file metadata backup and each actual object's96-byte header plus payload.
Counts include orphan images even though logical reads cannot see them. Object
scope markers, decoded models, capacity overhead, descriptors and copy scratch
are outside this encoded-image charge. A native component can be temporarily
copied before a budget refusal, so128MiB is not a whole-process peak-memory limit.
No descriptor/worker pool or process memory reservation is claimed.

Preflight observations are not leases. Captured file project/database/revision,
quota and physical/reference counts must still match them. Existing component
selection/inode/full-hash/WAL/graph proofs execute again around nested work.
An unacknowledged WAL tail retains the old component semantics; capture does not
turn it into a confirmed transaction or repair it.

## Failure and compatibility

Any source failure withholds the entire result; no successful earlier component
escapes. Native file source failure poisons that owner. Invalid caller ordering,
membership, count or byte budget does not poison healthy sources. Internal test
callbacks can return application errors without changing that distinction.
No source entry is repaired or removed, and no destination is selected.

The existing account bundle and paired native archive formats remain intact.
The copied v2 declaration is produced from explicit selected owners; it is not
written to root.json. Ordinary AccountRoot still refuses a v2 filesystem root.
This API does not prove files reside under one common parent, encode a joined
archive, publish a backup or restore all components as one destination. Independent
component restores are covered; common atomic restore is still required.

Current user permissions, file endpoints, whole-process resource reservations,
deployment and production readiness remain open. Use synthetic data only.

## Verification

The [source-bound report](measurements/2026-10-15-component-capture/verification.json)
records frozen sources, exact commands/results and earlier failed fixture attempts.
The complete workspace passes1764 cases per Rust1.99/1.89, with43 ignored entries
(39 parent-invoked child fixtures and four opt-in corpus generators). SDK passes34
unit and12 actual local-server/restart cases. Full builds, strict workspace/fuzz
lint, formatting and minimum fuzz compilation pass. All640 frozen input hashes
match after every final process closes with exit0. No new sanitizer/container or
production acceptance is claimed; hosted prior/base results are separate.
Tests inspect original locks and actual referenced/orphan descriptors at the deepest
callback, verify descriptor return while copies live, exact source histories and
encoded charges. Admission tests cover zero/four/five components, reversed and
duplicate scope, nonprivate scope, actual128/129 physical objects, exact/shortened
image limits and all fifteen pairs of six global database identity positions.

Source tests replace original referenced/orphan objects on either side, replace
private/public WAL during file work, alter file directories/marker/WAL/privacy/
links/content, and require withheld results. Healthy peer components remain usable.
Fresh independent restore preserves copied rows/files while subsequent source and
copy writes diverge. Old copied sessions are revoked by the existing restore reset,
while original sessions remain valid. Sixteen generated small mixed models exercise
data writes and file revisions. Twelve real signal9 kills span admitted/account-
copied/all-retained/returned boundaries for empty, v1 and compacted v2 histories.
