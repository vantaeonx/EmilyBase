# Fresh native file root initialization

`initialize_file_root(fresh_destination, project, quota)` creates one complete
standalone native metadata/object pair from scratch. It uses the original engine's
WAL initialization, the original object scope-marker publisher and the exact native
file catalog schema. It does not manufacture an empty backup as a bootstrap.

```sh
emilybase file-root-init new 01010101010101010101010101010101 --max-objects 8 --max-bytes 32768
```

The operator chooses a native path, an explicit canonical project and physical
quota. Invalid CLI values refuse before any entry is created. Zero quota is valid
and forbids physical publication. Existing destinations are never overwritten.
This is not a server project, account/user grant or a production-ready file service.

## Original ownership and durability

A private common0700 directory stage retains its original parent/root. The new
`Database::create_at(parent_descriptor, single_leaf)` delegates to the same original
WAL/snapshot initialization and fsync/readback rules as path-based creation. Empty,
dot, dot-dot, slash, NUL and nondirectory parents refuse. Native descriptor creation
intentionally follows the original retained parent across moves, while existing
path entry points keep their path checks. The returned Database's internal native
cache path uses its own retained directory descriptor, remaining valid after the
caller closes the parent. Checkpoints, compaction and primary cache save/load keep
using that original owner. No engine/lock descriptor is publicly exported.

`Database::check_directory_at(parent, leaf)` observes that the exact original
directory and WAL remain selected. Initialization holds both actual child-directory
descriptors. Object marker initialization opens an independent description of its
retained original child, avoiding a shared-clone lock lifetime. Metadata schema,
project/database binding and quota commit in the original engine. The initial root
contains exactly `metadata` and `objects`, no references or physical objects, and
its acknowledged native WAL1 transaction is2.

Before common selection, the original restored-pair guard verifies actual child
identity/private modes, original WAL descriptor/bytes and exact two-child roster.
Original Database and ProjectDirectory owners remain locked. A fresh coordinated
snapshot must exactly match the initial committed metadata/project/quota and empty
physical graph. After original no-replace common selection and directory/parent
fsync, these checks repeat against the selected root before success.

Nonempty failed private stages remain for operator inspection. Common selection
uncertainty or any later failure is OutcomeUnknown; selected roots are retained,
never repaired, recursively swept, overwritten or blindly retried. CLI stdout
failure after durable initialization likewise leaves the complete selected root.
Counts/identity output uses the same [paired archive report](native-file-archive-cli.md),
omitting names, owners, payloads and credentials.

Native ancestor trust, descriptor/proc availability and finite final observations
remain. This provides no namespace lease, global memory/FD admission, current user
policy, AccountRoot coordination, HTTP file route or signed URL. No existing format,
private schema version, transaction ACK rule or external dependency version changes.
See [ADR0143](adr/0143-original-engine-common-file-root-initialization.md).

## Executed evidence

[Source-bound checks](measurements/2026-10-10-file-root/verification.json) record
native original-parent moves before/during initialization, caller-handle closure,
later checkpoint/compaction/primary-cache save/load and exact acknowledged reopen.
Invalid leaves/parents and existing names create no substitute database. Same-byte
child substitution and before/after directory/parent fsync failures refuse success
and preserve inspectable original state. Existing path-based checks also rerun.

Fresh zero/nonzero quota roots preserve exact database identity/initial history,
private two-child layout, both kernel owner locks and exact descriptor-count return.
Later valid writes persist; quota refusal preserves the initial WAL. Twelve child,
WAL, scope-marker and root-roster changes before/after common selection refuse,
leaving preselection stages private or preserving uncertain selected roots.

Twelve actual SIGKILL cases cover metadata-only, object marker, catalog committed,
owners admitted, selected and caller-success with zero/nonzero quotas. Initial
creation uses WAL1 only. Selected/acknowledged roots reopen with transaction2;
nonzero quota roots accept transaction3. Unselected partial stages never appear at
the common name. Thirty-two generated quota/payload models compare actual admission,
physical charge and backup/restore against independent expected state. Actual CLI
processes verify relative paths, exact count/identity/quota output, malformed values,
no-replace preservation and failed stdout delivery. Physical power loss and all
production release gates remain unfulfilled.

Final affected matrix:652 checks per Rust1.99/1.89 (291 native files/storage/CLI,
325 database/WAL/transactions/backup and36 account/Root/registry regressions), with
614 unchanged source hashes afterward. Workspace/fuzz formatting, strict all-target
lint, affected builds and minimum fuzz builds pass. Twelve new and131 preceding
native process kills pass per toolchain. The separate hosted allocation-diagnostic
failure on cc06efe remains under investigation; this matrix does not include that
opt-in diagnostic or constitute full-workspace production acceptance.
