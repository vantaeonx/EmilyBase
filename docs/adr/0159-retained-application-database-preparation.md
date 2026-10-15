# ADR0159: move the actual restored database through trusted application preparation

Status: accepted for experimental synchronous native Linux core coordination.
Private account adapter and whole account/file restore remain open.

## Decision

Add with_prepared_owner_bytes_at with a trusted factory receiving the actual restored
Database by value, a PreparedDatabaseOwner wrapper and readonly checked work. Keep
the original creator WAL descriptor and directory through factory append commits,
checkpoint, no-replace child selection, parent fsync and final coordination proofs.
Accept a new expected baseline only after exact acknowledged readback from that same
original inode with the complete source prefix preserved. Forbid compaction/rebinding.

Add a finite native directory-descriptor comparison to Database; compare actual
directory/WAL ownership without exporting a handle or granting user permission.
Keep original supplied handles across parent moves. Reject a foreign directory even
when its logical identity/history match. Preserve healthy original state on a foreign
comparison. Existing engine and ordinary backup restore protocols stay unchanged.

Use a private adapter for private application stores. A public trait implementation
on AccountStore would expose mutable storage outside its invariants and is not the
planned integration. Factories must keep the supplied original owner and must not
detach/export it through errors or other channels; trusted native factories are not
adversarial or sandboxed. Actual account snapshot validation/session reset belongs
to the later private adapter.

## Consequences

Application reset can eventually append its durable state without closing the owner
before child/common selection. Work results wait for original physical/history/name
proofs and confirmation syncs. Callback error values require propagation of both
layers. Every late failure is RetainedRestoreUnknown; nonempty/foreign/selected paths
are preserved, never swept or blindly retried. A committed callback write cannot be
rolled back by the coordinator and is preserved while confirmation is withheld.

Original formats, acknowledgement semantics and external dependencies do not change.
Additional complete expected WAL/replayed models/descriptors are bounded per component
without a whole-process reservation. Native OS-owner trust, finite observations and
separate application/global-resource gates remain. No private account reset, common
root restore, new parser sanitizer or production readiness is marked complete.

Verify actual transfer/lock lifetime/drop, same-identity foreign owner refusal,
factory and callback errors/commits, physical substitutions/tails/privacy/links,
pre/post-selection sync failures, independent rows/history models and actual kills.
See the [contract](../prepared-owner-restore.md) and final source-bound evidence.
