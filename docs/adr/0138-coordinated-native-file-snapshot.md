# ADR0138: capture native file metadata and physical objects together

Status: accepted for immutable native capture; common publication/restore pending.

## Decision

FileStore owns the original synchronous metadata database and object directory.
Capture requires its mutable borrow, verifies the complete schema/scope/reference/
quota graph and retains every actual physical object descriptor, including orphans,
through export of the exact acknowledged original WAL and copying of the complete
object inventory. Replay the metadata backup and apply the same exact private-schema
validator as live opening. Compare its quota/references with the source graph.

Before returning the immutable pair, reverify all retained descriptors, original
descriptor-relative object scope, complete inventory and exact acknowledged
metadata WAL. Moving an original owner directory does not rebind it to a replacement
at its old path: capture continues from its retained original descriptors. This is
not certification that the old top-level source pathname still selects that owner. A
post-admission failure returns no image and requires FileStore reopen. Sources are
never repaired, retried or changed. The snapshot owns sensitive metadata backup
bytes and native immutable object images, not descriptors or account authority.

## Consequences

Valid physical orphans remain in the image and quota accounting. Source writes
through FileStore are excluded during capture; native trusted filesystem/ancestor
assumptions and the finite final-observation boundary remain. Identical restored
copies do not acquire unique ancestry from matching database/project identities.

At most128 original object descriptors are retained locally. Existing64 MiB WAL
and64 MiB physical payload bounds do not represent a whole-process memory budget:
replay, copies, headers, cache and scratch space also cost memory. Server admission
must be designed separately. This is a native operator API outside current Root,
HTTP, user file policies and signed URLs.

This change introduces no archive wire format and changes no original database,
WAL, object, archive or private-schema versions. Persisting two independent image
files is not atomic common backup publication. Component round trips exercise the
existing restorers; a checked combined format, common no-replace publication,
common restore and their crash/fuzz campaigns remain explicit future gates.
