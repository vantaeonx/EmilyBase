# ADR0153: retain every original payload through native snapshot coordination

Status: accepted for experimental synchronous native Linux coordination.
Combined account/file runtime and archive/restore remain open.

## Decision

Add `NativeFileRoot::with_snapshot(FnOnce(FileSnapshot) -> T) -> Result<T>`.
Use the existing capture/format/graph proofs while retaining every actual original
physical object reader across work and a second full source proof. Include orphans.
Keep ordinary capture's previous behavior/bytes without the additional work pass.
Narrowly borrow common selection around the callback and compare final root state.
Withhold/drop values and poison on actual source failure; repair/adopt nothing.

Treat a callback error as its chosen value, rather than assuming one concrete error
type. This permits checked nested components without coupling file storage to server
error enums. Require callers to propagate both nested result layers and defer
publication until all outer source proofs succeed. Expose no original descriptor,
mutable component or user/file authority.

## Consequences

Two independently returned immutable copies cannot establish this longer original
file lifetime. A tested capture-then-fresh-inspection prototype accepts an identical
object replacement during later work. The new boundary catches that replacement
because its exact original readers remain held. This extends a coordination contract
without claiming the original single-capture contract was defective.

Retained descriptors, repeated full hashes and caller nesting have resource costs;
per-store caps do not reserve a combined process budget. Future joined collectors
must admit aggregate counts/bytes/FDs and current source scope separately. Existing
archive format/restore remain applicable to successful copied values. No live v2
account root, common publication, user endpoint or production gate is enabled.

Tests cover actual original payload descriptor lifetimes/return, orphans, nested
components and error propagation, same-byte replacement, source corruption/privacy/
links/namespace changes, normal/relative parent behavior, maximum readonly payload,
fresh independent restore, generated models and actual coordination kills. Strict
stable/minimum native/core/CLI regressions precede publication; finite observations
and trusted native caller assumptions remain explicit.
