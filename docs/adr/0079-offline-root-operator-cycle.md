# ADR0079: Explicit offline root backup and restore commands

Status: accepted for experimental offline tooling; production gates remain open.

Expose the tested single-root protocol through real operator commands:
`account-bundle-restore BACKUP TARGET --reset-at SECONDS`, `account-root-verify ROOT`
and `account-root-backup ROOT TARGET`. Require explicit bounded trusted reset time;
never infer it from an archive or silently choose a service clock. Invalid values
must fail without echoing supplied text, before archive reads or staging.

Root capture uses the exact canonical manifest roster and the same retained-owner
inventory path as inspection. Copy each validated private image while every private
and data owner is held, then verify final source identities/manifest before encoding
and publishing the immutable EMILYBND image. Capture preserves live session scopes,
credential state, API keys and acknowledged histories; restore resets private scopes.
Do not discover or append independently existing private stores.

Reject destinations inside the source root and preserve existing targets. Reuse the
owned 0600 file publisher, full readback, no-replace rename, parent fsync and typed
post-rename uncertainty. The root source is exclusively captured; two publishers
can race after their separately completed captures without weakening source locks.

All CLI output is aggregate counts plus archive size or operator reset time. Never
print IDs, project/user names, rows, digests or access/refresh credentials. Password
pool capacity is fixed to one for this offline tool; no password input is accepted.

This closes an explicit offline backup/restore/re-backup cycle. It does not create
an authoritative attached-service catalog, HTTP account worker, roles/RLS, streaming
or encrypted backups, whole-process memory admission or production readiness.
