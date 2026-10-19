# ADR0165: capture and back up all internally owned v2 components

Status: accepted for experimental native capture and operator backup.

## Decision

Add AccountRoot::capture_v2, backup_account_file_root_v2 and explicit CLI
account-file-backup-v2. Use the exact complete original v2 manifest and internally
held native file owners; never select a subset, adopt a directory roster, reopen
components or reinterpret a v2 root as v1. Keep legacy v1 capture refusal intact.

Split original file topology from mutable file models. Nested native snapshots
enclose checked account/public capture and publication, retaining original payload
readers through their native final proofs. Full AccountRoot readiness checks all
actual file selections again after nested borrows end. The selected output file
remains retained through source unwind. Reuse the existing canonical joined format,
publisher, outside-directory guard and late-uncertainty protocol.

Represent copied account data as AccountComponentSnapshot, without a whole-root
manifest. Existing v1 AccountRootSnapshot wraps this component and its true v1
manifest. A full joined snapshot alone carries v2 bindings. This is an experimental
Rust API type change, not a persisted format change.

A failing regression demonstrated that committed private-history equality misses
an external unacknowledged WAL append during publication; an expanded case reproduced
the same public-WAL failure. Reuse bounded original
physical-tree readers for joined readonly v1/v2 capture, checking full bytes and
selection after account/public unwind. Preserve external bytes and selected output;
withhold confirmation after an observed late failure. Plain v1 committed-prefix
capture retains its documented suffix-exclusion behavior; joined publication has
a separate stronger internal entry point.

## Consequences and acceptance

No source session reset, clock change, format migration, provisioning, current
user-file permission or encryption. Explicit file binding and global identity/count
admission remain mandatory. Bounds are separately documented for physical readers,
encoded copies, transient buffers and models. Native/public/payload lifetimes are
different and tested at their actual boundaries, not extended by copied metadata.

Verify exact full manifests, sessions/source bytes, independent restored writes,
both WALs, excessive object admission before capture, original-parent movement,
physical mutations/replacements, source-contained/existing outputs, fsync failure,
actual killed workers before selection and after actual API return, generated
models and actual backup/verify/restore CLI. See the [contract](../v2-root-backup.md).
Ordinary binary v2 startup and remaining production gates are not completed here.
