# ADR0078: Restore selected data and private stores under one owned root

Status: accepted for experimental offline restoration; platform acceptance remains open.

EMILYBND already captures a common registry/private boundary. Restoring its members
to separately visible operator destinations would expose only part of that state.
Restore into one0700 owned stage with `registry/`, `private/` and a0600 canonical
`root.json`, reset every selected private session scope before root publication,
then sync and select the complete root using the existing no-replace protocol.

The experimental root manifest version1 declares the explicit sorted private
roster and trusted reset time. Its canonical JSON/CRC envelope is bounded to8192
bytes and128 project IDs. It grants no authentication authority. Missing private
stores are not discovered or invented. Generic project credentials are preserved;
API key rotation remains an explicit separate operation.

Reopen all private owners before capturing any data prefix, retain all data owners
through private validation, and compare exact registry bytes plus hashes of the
prepared private images before root selection. Root/child directory owners and
the canonical manifest must still match. Reject malformed sources before staging.
Before rename, clean only owned staging; after rename report publication uncertainty
and preserve the selected root. Process-killed stages are never automatically adopted.

Private reset requires WAL headroom; no implicit compaction is introduced. Trusted
local paths/time and Linux descriptor-owned publication remain deployment limits.
This is offline restoration, not automatic HTTP private-store discovery, an
authoritative service catalog, key rotation or a production acceptance claim.
