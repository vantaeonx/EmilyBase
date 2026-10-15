# ADR0151: capture while retaining every original account-root owner

Status: accepted for experimental native Linux v1 capture. Combined account/file
runtime ownership and archive publication/restore remain open.

## Decision

Add opaque copied `AccountRootSnapshot` and synchronous native root capture.
Refactor existing registry/private encoding behind an internal checked callback
without changing its old public callers or bytes. Hold all original public data
owners across the callback, alongside the root's original registry/private owners.
Preserve the existing serialized transaction and established lock-order rules.

Split narrow structural references from mutable registry/private borrows so final
source checks still use the original owners. Check selected original directories,
WAL inodes/private modes/single links, root inventory and manifest. Compare fresh
acknowledged-prefix/private-image hashes with the copied history before release.
Use the engine's original finite directory/WAL selection check through a narrow
AccountStore wrapper; expose no native descriptor or mutable private database.

## Consequences

Future file snapshots can execute inside an internal boundary that still holds
every account/public data owner. Returning an account snapshot and only then
capturing another component would not provide that boundary. This prerequisite
does not define a joined format, current file authority or atomic joined restore.

Copies retain sensitive bytes and declared metadata but no live owners. Existing
v1 restore remains applicable and resets only restored sessions. Snapshot capture
does not write/advance clocks or copy abandoned WAL tails. Active/count/encoded-byte
bounds remain; fresh final exports add work without a numeric whole-heap guarantee.

Regression tests first reproduce acceptance of byte-identical substituted WAL
inodes and nonprivate WAL modes, then verify refusal without repair. Other tests
cover original owner lifetimes, exact source history, independent restore/source
mutation, namespace changes, acknowledged history changes, unacknowledged tails,
generated rows and actual killed capturers. Existing root/registry/auth/core/CLI
regressions must pass on stable and minimum Rust before publication. Native finite
observations, trusted caller and experimental deployment limits remain explicit.
