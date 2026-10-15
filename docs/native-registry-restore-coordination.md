# Retained native registry restore coordination

`with_restored_registry_backup_bytes_at(bytes, parent, leaf, work)` verifies one
existing registry image, privately prepares it under an original native parent and
selects one complete fresh child without replacement. Trusted synchronous work then
runs while the original registry/project/data owners and every public database writer
remain held. Its callback receives copied RegistryBackupReport metadata, including
project identities/names/counts; no key digest, row payload or mutable owner escapes.

## Preparation and original ownership

Decode the complete existing bounded image before staging. Require an actual parent
with exact0700 permission/special bits and one1..255-byte leaf, without slash, NUL,
dot or dot-dot. Prepare the unchanged original metadata/WAL/checkpoint components,
then open the original registry through the retained stage descriptor. Moving that
native parent does not adopt a different directory at its previous pathname.

Retain one independent original data-directory/WAL reader per project before taking
all public database writer owners and copying the first acknowledged prefix. Require
exactly the verified project roster; additional entries, including normally tolerated
incomplete creation names, refuse. Each original WAL must remain a regular single-link
600 file with exact source length, inode, mode, link count and timestamp stamps.
Compare all actual original bytes against the verified source in8192-byte chunks,
with before/after metadata and visible-name proofs. Directory identities must match.

The original checked registry capture also compares the full canonical registry image
against the verified input, retains every public writer through work, and repeats exact
acknowledged-history hashes and normal project metadata/directory checks afterward.
Project JSON keeps its existing canonical-model/checksum semantics. Original WAL
readers add a physical-file contract; they do not change the ordinary engine's rule
that an unacknowledged tail is excluded from its acknowledged export/recovery state.

Repeat physical proofs before child selection, after parent fsync, and at the final
selection observation after all original history checks. Keep every writer alive
through that last observation. There is no close/reopen interval across caller work
or an enclosing common root's selection. A same-byte replacement is not accepted as
the prepared original. Original formats, API keys, identities and WAL1/2 are preserved.

```mermaid
flowchart LR
    V[Verify complete image] --> P[Prepare private registry]
    P --> O[Hold all original writers and WAL readers]
    O --> S[Select child and sync parent]
    S --> W[Nested work and enclosing root selection]
    W --> H[Verify original histories and physical WALs]
    H --> N[Check final selected name]
    N --> R[Release owners and return]
```

## Enclosing coordination and outcomes

Work starts only after complete no-replace child selection and containing-directory
fsync. If multiple components are being assembled, keep that containing root private
until all preparations succeed. Work may nest another checked component and publish
the enclosing root. The original public writers remain held throughout that nested
work and final outer proofs. All callback values wait for the verified outer result.

The return type is Result<T>. A callback Result is an ordinary nested value, requiring
explicit propagation of both layers. Healthy application refusal leaves the already
selected child intact. Any observed failure after child selection returns
RegistryRestoreUnknown; the child or enclosing root may already exist durably. Preserve
selected/foreign files, inspect explicitly and never overwrite, delete or blindly retry.
Before selection, failures leave the target absent; partial private stages remain for
operator inspection. Existing files, directories and aliases are never replaced and
never run work. Trusted callback unwinding releases original owners without a receipt.

An enclosing coordinator must separately admit global account/file scopes and resource
limits, reset restored private sessions, verify its own complete root manifest/layout,
and propagate uncertainty after common selection. This function checks a registry,
permits independent copies of the same project scopes and grants no current user or
service authority. It does not perform account preparation or enable v2 startup.

Native Linux parents/OS operators remain trusted; checks are finite observations,
not namespace leases. Existing128-project/128MiB image bounds apply. Source/copy images,
replayed models, per-project directory/WAL readers and writer owners consume additional
memory/descriptors, without a whole-process reservation or network deadline. Integrity
checks do not authenticate backup origin or supply encryption.

## Verification boundary

[ADR0158](adr/0158-retained-registry-restore-coordination.md) records the decision.
[Source-bound verification](measurements/2026-10-15-registry-restore-coordination/verification.json)
records final tests and exact source hashes. The first prototype only compared committed
history; an isolated test reproduced Ok after an external unacknowledged tail append.
The retained original physical-WAL proofs correct that new coordination defect without
changing ordinary recovery. Preliminary failures are excluded from final counts.

Tests exercise nested registries through common selection, actual writer/WAL lifetimes
and release,128 public writers, callback refusal/unwinding, moved parents, invalid input/
leaves/privacy, no-replace collisions,24 late mutations across single-WAL1 and mixed-
WAL1/2 sources, four additional permission changes, six injected sync failures, a
preselection tail change and an identical-byte final root substitution after history
validation. Sixteen generated rosters compare exact source images, preserved API keys,
row values and WAL metadata against independent models.

Eighteen actual signal9 kills cover all-public-owners-admitted, child-selected,
child-confirmed, enclosing-common-selected, final-selection-check and returned
boundaries for empty, one-WAL1 and mixed-WAL1/2 sources. Actual original locks/readers
span every pre-return boundary and are released after return. Private preparations or
complete selected roots reopen independently and preserve history. Restored projects
accept the next write. Sources/archives stay byte-exact and a fresh target still works.

Full account/file restore, explicit v2 adoption, user file permissions, physical
power-loss and production acceptance remain open. No new parser sanitizer campaign
or SDK/container execution follows from this filesystem coordination increment.

Final affected suites pass1220 cases per Rust1.99/1.89:305 complete server-library,
147 auth,28 named registry integrations,42 account CLI,325 original engine,
328 storage/object/file and45 native CLI.36 ignored entries are31 parent-invoked
process fixtures and five opt-in corpus generators. Every final process closes with0,
all651 source inputs match, and formatting/strict workspace/fuzz lint, builds and
minimum fuzz compilation pass. The increment adds924 net Rust lines with tests;
total source is139035 lines. Preliminary attempts are excluded from these totals.

Earlier hostedf588be3 independently passes1788/0/44 on each complete workspace
toolchain and all five jobs, with648 committed source hashes checked. Its separate
allocation diagnostics pass58 cases outside those workspace totals. This earlier
complete CI belongs to that earlier source, rather than this coordination protocol.
