# Offline public admission administration

Use the actual emilybase binary against an existing **stopped** private root.
The root owner refuses concurrent selection by the server or another process.
This controls the native user gateway; user-only HTTP is still pending. Use only
synthetic data. Production readiness is not claimed.

```text
emilybase account-admission ROOT PROJECT --key-file PRIVATE_KEY_FILE enable-catalog
emilybase account-admission ROOT PROJECT --key-file PRIVATE_KEY_FILE status
emilybase account-admission ROOT PROJECT --key-file PRIVATE_KEY_FILE open --expected REVISION
emilybase account-admission ROOT PROJECT --key-file PRIVATE_KEY_FILE close --expected REVISION
```

ROOT and PROJECT come from trusted operator configuration/metadata. The key is
read through the shared private-file loader: exact64 lowercase hex bytes with
optional single LF, regular file0600/0400, one link, no symlink/FIFO. It is never
accepted as a command value or printed. See [key publication](offline-service-keys.md).
All actions reauthorize the current service key under the actual retained owner;
rotation or a foreign project's key refuses without changing admission.

First explicitly enable the [policy catalog](policy-cli.md). enable-catalog then
migrates v4 to v5 in the original atomic transaction with admission **closed**.
It does not automatically install a table policy. A repeat on existing v5 returns
its current receipt without closing or opening it. status/open/close never migrate
or change the session clock. Existing roots remain closed until explicitly opened.

Successful output contains only metadata, for example:

```json
{"private_version":5,"admission":{"enabled":false,"revision":"12","previous":"0"}}
```

Revisions are actual private WAL commit identities, not sequential flag counters.
Keep the entire returned decimal string. Open/close require canonical u64 digits:
no sign, leading zero, whitespace, exponent or overflow. The original reserved WAL
maximum refuses. Invalid command values refuse before key loading or root selection.
The CLI never logs keys, passwords, session tokens or table contents. A returned
receipt is operator metadata; it cannot authenticate a user request.

Use the current revision for a change. An identical retry with the recorded
predecessor or current revision is readonly; arbitrary older revisions refuse.
An earlier opening command cannot undo a subsequent close. Never automatically
retry a conflict or a lost result with a newly fetched revision.

Closing suspends all native public login/refresh/logout/metadata/row calls without
revoking all session families. An intentional reopen can resume a still-current
unexpired token. Use [user revocation](user-cli.md) or the original session controls
when credentials must become invalid permanently. Service-key rotation is
independent of a current public user session.

Verified nonempty private/common-root restore closes an enabled copy and changes
session incarnation before publication. Inspect its new status and explicitly
open with that revision when intended. Users must log in again: source tokens
remain invalid in the copy even after opening. Source data/sessions stay unchanged.
Generic engine restore is a lower-level operation and still requires the documented
private reset before traffic.

The native mutation can commit even when stdout fails. The command reports
`admission output unavailable; inspect current state before retry`. Inspect status
using the current key. Only the original exact-retry contract can establish a
readonly retry; terminal failure does not mean rollback. This increment adds no
process-kill or power-loss claim; original native recovery tests remain separate.

Public HTTP, signup, roles, user SDK/dashboard, resource/load/upgrade and independent
security/production gates remain pending. See
[ADR0112](adr/0112-offline-public-admission-cli.md).

The subsequent [public session HTTP adapter](public-session-http.md) now consumes
this explicit flag for admitted user sessions. Public user-only row HTTP remains
pending; opening a flag does not install or bypass a table policy.
