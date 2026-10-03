# ADR 0019: pin project directory identities for live capabilities

Status: accepted after failing local namespace-replacement regressions.

## Context

The registry held an exclusive root inode but selected later files by pathname.
Moving that root and installing another private directory let creation publish
outside the actual owned inode. Similarly, an accepted scoped capability could
follow a replaced project/data directory. Private permissions alone cannot detect
these accidental namespace changes. File-name-based ownership was insufficient.

## Decision

Hold no-follow, close-on-exec directory handles for the root and each project's
project/data directories. Compare private mode and device/inode identity before
registry listing, creation, rotation, capture and scoped execution/explain/status.
Capabilities retain all three handles through operation completion. Project/data
handles pin inode lifetimes, preventing deletion/reuse from silently matching an
old identity. They do not acquire additional database ownership locks.

Keep authorization itself pure and bounded: digest checks and handle clones run
without filesystem work. Directory checks happen inside the synchronous operation,
after its request gate, on the HTTP blocking worker. Thus network reactor behavior
and existing worker/ownership lifetime guarantees remain intact.

Creation pins private staging directories before rename. Rotation syncs its
retained project handle. WAL/checkpoint inode replacement inside the same directory
remains supported. A changed directory fails with a typed private-path error;
the HTTP layer returns a generic storage-unavailable response. Reopen explicitly
to operate on intentionally moved/replaced directories.

## Consequences and limits

No stored format or HTTP route changes. Each committed project adds two directory
descriptors, bounded by 128 projects; capabilities share handles through Arc.
Offline capture still takes every actual database owner and refuses active
capabilities. A replaced project/data directory fails locally while healthy sibling
operations remain available. Listing validates all known directory anchors.

Tests first reproduced the defects, then covered all three namespace levels and
actual HTTP generic failures, retained old/replacement bytes, sibling availability
and intentional reopen. The checks target accidental changes under a trusted
local operator. They do not sandbox a malicious same-user/root administrator
or claim race-proof protection against continuous adversarial filesystem mutation.
Such a boundary needs a broader descriptor-relative storage design and audit.
