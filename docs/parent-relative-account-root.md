# Parent-relative native account roots

`AccountRoot::open_at(&parent, leaf, pool)` opens an existing v1 account root
relative to a trusted original private directory descriptor. It duplicates and
retains that parent, opens the exact selected child without following its final
symlink, acquires the original root lock and moves the existing inspected registry/
private owners into the service. It creates no missing member or schema, migrates
nothing and advances no private clock. Ordinary `open(path, pool)` keeps its current
absolute-path selection behavior.

The leaf is nonempty UTF-8, at most255 bytes, with no slash/NUL and no dot/parent
component. The parent must be a private real directory. Invalid inputs refuse
before child admission. Missing, busy, foreign, unsafe or oversized rosters keep
existing refusal behavior; active private count is still4, offline count128.

```rust
let parent = std::fs::File::open("container")?;
let root = emilybase_server::AccountRoot::open_at(&parent, "accounts", pool)?;
drop(parent);
let projects = root.projects()?;
```

The containing directory may move without invalidating the original relative
selection. A byte-identical replacement at the old absolute location cannot redirect
operations. Renaming/replacing the selected child within the original parent still
refuses. This is descriptor-relative component ownership, not proof that a displayed
absolute parent path currently selects that directory. A future common-root owner
must retain/check its own external namespace separately.

## Paths, selection and drop order

Component registry paths keep the selected leaf hierarchy relative to the retained
parent, instead of canonicalizing it to a stale absolute path. The internal registry
opener is readonly and creates nothing. Original registry/project/data directory
owners are still compared with the visible selected paths. Root structural checks
also compare the original registry under the actual original root descriptor.

An owned registry descriptor alone would hide replacement of its containing name
after service admission. The final design retains selection-relative paths through
the original data capability checks and database opening. A dedicated late-boundary
test replaces root/registry/project/data names after service admission but before
data opening; no staged mutation or WAL change occurs. In a private rejected-prototype
copy, the own-registry-descriptor-only variant fails that test and writes despite the
root substitution. The main source remains unchanged by that reproduction.

The public root exposes no registry, data owner or detached AuthorizedProject.
Internal descriptor paths remain inside the original root's lifetime. Registry/
private contents drop first, then the child root owner, then its retained parent.
Dropping the caller's parent handle cannot invalidate paths or release a caller-held
parent lock while the root still owns the same open-file description. This method
does not itself acquire a new parent lock or prevent a trusted caller explicitly
unlocking its own descriptor.

`check_directory_at(&parent, leaf)` performs existing root readiness checks around
the supplied private parent/leaf identity check, without exporting an owner or
rebinding the original source. Another handle selecting the same original parent
works; a copied root, symlink, bad leaf or foreign/missing child refuses. A failed
comparison against caller-supplied metadata does not grant authority or mutate the
healthy original root.

Registry backup output containment resolves the physical source path before prefix
comparison. Descriptor aliases therefore cannot allow output inside the live registry.
Existing no-replace/verification/publication rules remain. This does not add a live
combined account/file backup or a new root capture API.

## Evidence and limits

[Source-bound checks](measurements/2026-10-15-account-root-at/verification.json)
cover caller release, moved/replaced parents, original SQL routing, exact history,
session/time/refresh preservation, original parent/root/private/registry locks and
descriptor return. Invalid/busy/missing/nonprivate input preserves source. Sixteen
selected-child/manifest/private/registry/parent changes refuse on both WAL modes;
eight later root/registry/project/data substitutions refuse before data opening.
The4/5 active boundary is checked before registry admission. Sixteen generated
roster/move models cover explicit0–4 private members and independently verified scope.

Six actual SIGKILL cases span original service-opened and readonly root result,
empty membership plus nonempty WAL1/2. Parent/root/children remain held until kill;
reopen preserves history and permits an independent credential operation. Native
worker termination checks the actual signal9. Relative backup tests refuse internal
targets and verify external capture/restore without modifying source. Existing root,
registry, CLI, core and SDK regressions are recorded separately from the prior complete
1705-test source.

Ancestor configuration/native input remains trusted; checks are finite observations,
not filesystem leases or a sandbox against a hostile administrator. Private caller
proofs/current admission remain the original service rules. No filesystem/schema/
bundle version, durability order, external dependency or credential protocol changes.
The v2 file-capable manifest is still inspection-only and refused by ordinary/at
startup. Common account/file ownership/capture/restore, user file policies/HTTP,
signed URLs, resource admission, physical power loss and production gates remain
open. See [ADR0150](adr/0150-retained-parent-relative-account-root-ownership.md).

The later [live v1 capture API](live-account-root-capture.md) reuses this retained
parent/selection lifetime while holding every original public/private data owner
through checked coordination. It does not adopt file-capable v2 runtime metadata.
