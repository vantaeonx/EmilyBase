# ADR0150: retain the parent across native account-root component operations

Status: accepted for experimental native Linux v1 root opening/selection checks.
File-capable v2 runtime adoption remains open.

## Decision

Add parent-descriptor/one-leaf account-root opening. Validate bounded leaf/private
parent, retain an owned duplicate, then move the exact existing inspected root/
registry/private owners into the service. Keep ordinary path opening unchanged.
Drop all contents and the child owner before the retained parent. Keep original
schema, session, time and active-count rules; startup never provisions or migrates.

Use an internal readonly registry opener that preserves selection-relative paths
under that retained parent. Continue comparing registry/project/data owners with
their visible selected paths, including the original registry under the actual
root descriptor. Do not substitute an own-registry-descriptor-only view: that loses
the selected containing name after service admission, as the executed rejected
prototype demonstrates. No mutable registry/owner/capability escapes the root.

Expose readonly original root selection checking under an explicit private parent
without rebinding it or exporting authority. Canonicalize physical registry source
for backup containment so descriptor aliases cannot bypass internal-output refusal.

## Consequences

Caller handle release and parent moves preserve the original component and do not
redirect SQL to a replacement absolute path. Child/root/registry/project/data name
substitution still refuses at original readiness/data-owner checks. Caller-held
parent locks remain with the retained open-file description until the root ends;
the helper acquires no independent parent lock or absolute namespace lease.

This is a native component prerequisite, not live file integration or a new capture
format. A common owner must guard its own top-level namespace, settle all owner/
resource/capture/restore boundaries and current file permission before enabling a
file service. Existing public/private credential gates and Linux/trusted-ancestor/
finite-check assumptions remain. File/schema/bundle/durability versions do not change.

Tests reproduce lost namespace selection in a private prototype, then verify the
final selected hierarchy, caller lifetime, original locks/routing, exact sessions/
history, substitutions before data open, physical output containment, independent
restore, generated rosters and actual killed readers. The prior complete workspace
result remains evidence only for its own source; affected checks certify this change.
