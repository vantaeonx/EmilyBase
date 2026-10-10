# ADR0141: verify and restore a native file pair under one selected root

Status: accepted for standalone native Linux operator restore.

## Decision

Verify the entire immutable paired archive before creating any stage. Create one
private owned0700 root and restore existing metadata/object components under it
using explicit retained-parent descriptor entry points and constant single leaves.
No parent pathname is resolved again for component destination selection. Existing
path entry points retain their visible-parent checks; descriptor entry points
intentionally keep the original parent across moves without adopting replacements.

Before common root selection, open/retain both actual child directories, original
restored metadata Database and ProjectDirectory, an independent actual readonly WAL
descriptor and every physical object descriptor, including orphans. Require exactly
two common-root children, exact private child identities, selected WAL identity and
bytes, scope marker, complete physical inventory and replayed reference/quota graph.
Keep all these owners through original no-replace root selection/parent fsync and
repeat complete checks against the actual selected root before success.

## Consequences

The original component formats, private schema1, identity and acknowledged WAL
prefix are preserved. No rescoping, real data, user permission or unique ancestry is
created. Metadata cache materialization remains disposable and uses existing rules.

Invalid archives create no stage. Pre-common-selection failure leaves the common
target unselected and preserves nonempty private stages for inspection. Common
selection uncertainty or any subsequent error is OutcomeUnknown; never remove,
repair, overwrite or retry a selected root. Existing destination entries refuse.
Source snapshots/archives remain unchanged. Native trusted ancestors and finite
final-observation boundaries remain; this is not a sandbox against administrators.

An integration test first reproduced ENOTDIR when a path-only component restorer
received a descriptor pathname whose trailing dot was normalized by parent parsing.
Explicit native owned-parent staging/restore APIs fix the boundary without relaxing
no-follow path rules or converting retained ownership back into mutable paths.

Native descriptor count and image bounds are finite, not global server admission.
Process-kill/source-substitution/generated-state tests define this increment's
evidence. Physical power loss, coordinated AccountRoot inclusion, current file
policies, signed URLs and production acceptance remain separate gates.
