# Parent-relative native file components

`NativeFileRoot::open_at(&parent, leaf, project)` opens an existing native
metadata/object pair under a trusted original private parent descriptor. It retains
a duplicate, then uses the existing exact child/WAL/scope checks and independently
locked original metadata/object owners. Caller release and parent moves cannot
redirect operations to a replacement at the old absolute location. It creates no
member, migrates nothing and grants no account or user file permission.

The UTF-8 leaf is nonempty, at most255 bytes, with no slash/NUL and no dot/parent
component. The parent must be a real directory with no group/other permissions.
Missing, foreign, unsafe or busy components refuse without provisioning or repair.
The longest valid multibyte leaf is tested; limits count bytes rather than characters.

Original `open(path, project)` preserves its absolute parent selection rules. It
still refuses a moved/replaced configured parent. The new entry point deliberately
selects a component under an original descriptor rather than promising that an
absolute display path currently selects that parent. A future common owner must
guard its own external root namespace and original declared roster separately.

## Ownership and operations

The owned file store drops before its retained selections. Its cache/object paths
remain anchored to the original child descriptors, so subsequent publication,
rename/removal, quota CAS, capture and borrowed payload streaming use the selected
original pair after a parent move. Selected child/root/metadata/object/WAL/scope
substitution still refuses. Relative mode checks the retained parent's privacy at
all original topology boundaries, including actual write/ACK and cursor checks.

No parent lock is implicitly acquired. A caller-held parent lock remains with the
shared open-file description through the retained duplicate. Caller handle release
does not end it while the root remains alive; explicit caller unlock still affects
that shared description. This adds no root-wide transaction or filesystem lease.

`check_directory_at(&parent, leaf)` makes a finite comparison against the exact
original root under supplied private metadata. It works for normal and relative
roots. Original-source graph/selection checks surround the comparison. A foreign
parent, copied root, symlink, bad/missing leaf or nonprivate supplied parent refuses
without poisoning a healthy source. An actual original-source failure still poisons
the root, even if an operator subsequently restores the namespace. Neither result
exports a descriptor, rebinds the source or grants current user authority.

Parent privacy failure after blob publication preserves the charged orphan. If
metadata has already committed, the reference remains acknowledged and survives
reopen. The operation withholds success and keeps the existing uncertain-outcome/
poison rule. Normal source recovery and exact same-inode WAL readback are unchanged.

## Scope

This is a native component prerequisite for future common account/file ownership.
It does not enable the v2 file-capable root, join account/file snapshots, change
archive/restore formats, expose user HTTP downloads or supply signed URLs. Existing
project/database binding, physical orphan charge, bounded payload reads, revision
CAS and independent fresh restore remain required. Trusted callers/ancestors and
finite source observations remain the native boundary; whole-process resource
admission, physical power loss, independent security review and production gates
remain open. See [ADR0152](adr/0152-parent-relative-native-file-component-ownership.md).

## Executed evidence

[Source-bound checks](measurements/2026-10-15-native-file-root-at/verification.json)
record673 affected cases per stable1.99/minimum1.89:303 native storage/object/files,
45 actual CLI and325 original engine/WAL/transactions/backup cases. Builds, format,
strict workspace/fuzz lint and minimum fuzz compilation exit0.636 frozen source
hashes match after every process closes. SDK/server/auth/fuzz/lock inputs remain
unchanged from the prior capture increment; no new SDK, sanitizer, full-workspace
or container run is claimed.

Twelve new relative cases include16 independent generated revision/name/payload/
move/fresh-restore models, caller lock retention and descriptor return, no implicit
parent lock, bounded invalid/missing/busy inputs and independent two-way restored
writes. Six moves during original owner admission preserve selected data.96 selected
source changes during admission and64 around copied results refuse. Six parent
privacy changes around publication retain charged orphans or acknowledged references
according to the actual commit boundary. Foreign comparison preserves healthy
source; original source failure poisons. Sixteen actual signal9 kills span selected,
metadata, owned and acknowledged states, both WAL versions and empty/8193-byte
payloads, with original locks/history/reopen. Existing211 file-related kills rerun,
for227 per toolchain. Preliminary test-only compilation refusals are excluded.
