# ADR0147: guard original common-root reference and quota transactions

Status: accepted for experimental native Linux operator APIs and CLI.

## Decision

Extend the existing fallible selection hook to the original FileStore rename,
remove and quota transactions. Verify common root/parent/children/scope/original WAL
before commit, accept only exact acknowledged readback from the same retained WAL
inode after actual engine ACK, then require final selection and complete graph.
Share this check/error handling with guarded publication. No alternate transaction,
WAL, writable owner exposure or component version is introduced.

Reference mutations keep exact logical revision CAS and original referenced reader
retention. Quota changes keep opaque project/database/global-revision CAS and all
physical readers through the original transaction. Validate unchanged inventory and
unrelated catalog metadata after metadata-only mutations. Equal current updates are
verified WAL-preserving no-ops; stale equal observations conflict.

Expose copied native file/quota observations and explicit stat/rename/remove/quota
CLI commands. Quota CLI requires the actual database identity and global revision;
it compares these with the scoped observation instead of manufacturing a CAS state.
Count-only output omits display names, owners and payloads. Report failure does not
undo acknowledged data or grant an overwrite/retry capability.

## Consequences

Removal only changes logical visibility. Physical orphans remain charged and are
included in coordinated archives. Precommit source refusal discards staged metadata;
once commit is attempted, later uncertainty preserves data as OutcomeUnknown. Source
failure poisons the wrapper. Invalid ordinary requests preserve usable unchanged
state. Reopen/inspection remain explicit; no garbage collection, repair or retry.

Standalone FileStore entry points preserve their separate pair contract. Native
common selection is not a filesystem lease, current user permission or authenticated
backup origin. Existing bounds/allocations/descriptors, Linux/proc assumptions and
finite check windows remain. AccountRoot, user-file HTTP/download/policies/signed
URLs, physical power loss and production gates are still separate work.

Executed tests cover source substitution, exact CAS/no-op/charge models, killed
writers, independent reopen/paired restore, descriptor return and actual CLI receipt
delivery. A prior hosted revision remains evidence only for its own source.
