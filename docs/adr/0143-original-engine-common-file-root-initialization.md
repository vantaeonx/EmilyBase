# ADR0143: initialize a common native file root using original engine owners

Status: accepted for standalone native Linux operator creation and CLI.

## Decision

Create a fresh private common root stage, initialize original-engine metadata and
the original object scope under retained native children, then commit the exact file
catalog/project/database/quota schema in the existing WAL. Do not substitute backup
restoration for engine creation and do not expose an engine owner descriptor.

Add explicit Database parent-descriptor creation with single-leaf validation, shared
original WAL/bootstrap/fsync logic and a self-owned descriptor cache path. Existing
path entry points retain visible-parent/child checks. A separate readonly native
check verifies that the selected leaf and WAL still belong to the original Database.
Moving a native parent does not switch to a replacement at its former path.

Retain actual metadata/object children, original engine/object owners and original
WAL across common publication. Reuse the existing common-pair guard and coordinated
snapshot validation to require exact original history/scope/quota and empty graph
before/after no-replace selection and parent fsync. Object owner initialization opens
an independent description of the retained original child, preserving lock lifetime.

Expose file-root-init with required project/quota arguments. CLI result delivery is
part of process success, not permission to undo or overwrite a durable root.

## Consequences

Fresh standalone roots now have an operator entry point from scratch. Initial
history is WAL1 transaction2: original database initialization then private catalog
initialization. All component formats/schema versions and ACK rules remain unchanged.

Invalid quota/project input refuses before staging. Nonempty private stages remain
on failure; uncertain/selected roots are preserved. No repair, recursive cleanup,
rescoping, user authorization or real-data migration occurs. Native ancestor trust,
Linux descriptor/proc paths and finite final-observation boundaries remain.

Native parent/child substitution, original cache-path lifetime, injected fsync
failures, process kills, generated admission models and actual CLI output are tested.
AccountRoot/user file integration, server-wide resource admission, physical power
loss and production acceptance remain separate work.
