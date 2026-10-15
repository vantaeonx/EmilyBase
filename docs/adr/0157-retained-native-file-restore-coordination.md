# ADR0157: retain prepared native file owners through enclosing restore selection

Status: accepted for experimental synchronous trusted native Linux coordination.
Whole account/file restore and v2 runtime adoption remain open.

## Decision

Add with_restored_file_archive_at over an explicit original private parent and one
bounded fresh leaf. Verify the complete existing paired format before staging;
reuse original descriptor-relative child restoration, readback and no-replace
selection. Require exact0700 containing permissions, and check them through the
coordination boundary. Preserve ordinary path-based restore behavior by sharing
the original retained pair implementation rather than duplicating a new protocol.

After complete child selection and final proofs, run trusted work while the same
metadata/object writer owners, actual child/WAL descriptors and every original
physical object reader remain retained. Repeat full identity/privacy/bytes/reference/
quota/inventory proofs after work. Withhold its value until those checks pass. A
callback error may be a value; callers propagate both nested layers explicitly.
Postselection failure is OutcomeUnknown and never overwrites/removes selected data.

## Consequences

Nested preparations can carry original owners through an enclosing common root's
atomic selection, without reopening/adopting file names between components. The
callback sees copied report metadata only, with no mutable storage/owner export.
An enclosing coordinator still owns aggregate admission, roster/global identities,
account reset and uncertainty propagation. This primitive permits repeated scope
copies and grants no user or project authority. Existing whole-pair standalone
restore was not required to retain its owners across later caller work.

Repeated proofs and retained descriptors cost IO/resources; per-component bounds
are not a combined process reservation. Native parents/OS operators remain trusted,
checks are finite and selected children survive application refusal or unwinding.
Formats, parsers and existing runtime startup do not change. The new common account/
file restore, explicit v2 adoption and production gates are not marked complete.

Verify actual nested owner lifetimes, all physical readers including orphans,
identity substitutions, privacy/link/content failures, callback result withholding,
independent subsequent writes, generated state models and actual process kills.
See the [contract](../native-file-restore-coordination.md) and source-bound evidence.
