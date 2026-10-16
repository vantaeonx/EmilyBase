# ADR0160: retain the actual private account owner through restore preparation

Status: accepted for experimental synchronous native Linux account coordination.
Whole account/file restore and v2 runtime adoption remain open.

## Decision

Move the actual restored Database into a privately validated AccountStore constructor
and a private PreparedDatabaseOwner adapter. Do not implement the mutable database
trait on the public AccountStore. Prevalidate project/time/image before staging and
expose only validated private report metadata to nested coordination.

Add a borrowed PreparedSourceGuard and additive guarded core factory API. Check the
original creator's physical source, full bytes and stage/parent immediately before
application preparation. An isolated failing mutation test demonstrated that ordinary
WAL tail trimming could hide an observed pre-reset external append; refuse it without
changing ordinary WAL/recovery semantics or the earlier trusted factory API.

Append one durable fresh-incarnation/time preparation transaction. Activate legacy
v1/v2 as v3; preserve v4 policies and close v5 admission in the reset transaction.
Keep exact users/verifiers/epochs/disabled state and historical families. Keep the
same original owner and WAL through checkpoint, no-replace child selection, nested
work, confirmation syncs and final physical/history/selection checks.

## Consequences

Copied old credentials are invalid while the original source remains unchanged.
The trusted restore timestamp can reset the copied watermark under its fresh scope.
Callbacks get reports rather than storage or user authority. Caller-owned enclosing
root selection and resource admission remain separate. Nested callback errors require
both result layers; late observed failure withholds values and preserves uncertainty.
No closing/reopening, compaction, automatic adoption, recursive cleanup or claimed
rollback of acknowledged preparation is introduced.

Verify all private versions/WALs, legacy family preservation, exact policies/verifiers,
actual original descriptor/lock lifetime, copied token refusal, independent sign-in,
physical mutations, bounds, generated records and dedicated actual kill boundaries.
See the [contract](../private-account-restore-coordination.md) and source-bound evidence.
