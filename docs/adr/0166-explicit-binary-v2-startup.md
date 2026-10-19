# ADR0166: explicitly select v2 in the ordinary server binary

Status: accepted for experimental native startup and actual binary recovery.
Container v2 recovery and production acceptance are separate gates.

## Decision

After complete internally owned v2 backup acceptance, add exact optional
EMILYBASE_ACCOUNT_LAYOUT=v1/v2 alongside EMILYBASE_ACCOUNT_ROOT. Omission preserves
v1 account admission. Reject conflicting data modes, layout without an account
root and empty/unknown/non-UTF-8 selectors before storage access. Preserve native
path bytes and use static configuration errors without supplied-value logging.

Dispatch to the existing AccountRoot::open or open_v2 on the blocking worker,
then assemble the existing account router. Do not probe manifests to pick a mode,
perform migrations, create missing components, reset sessions or grant file access.
The legacy registry path remains independent. Account Compose configurations
default to v1 only for an unset selector; the registry image target stays unchanged.

## Acceptance and consequences

Test actual compiled server processes through local HTTP: both WALs, empty/one/four
projects, acknowledged mutation, rollback, actual signal9 after ACK and before a
complete request body, restart, sessions, wrong keys, independent backup/restore,
full file bindings, orphan bytes, unsafe/mismatched input, busy file/root/listener,
non-UTF-8 native paths, master secret files, legacy behavior and no sensitive logs.
Use actual child termination status rather than treating any failure as signal9.

Only the binary selector and process fixtures change; library engine/format/parser
code is unchanged from the immediately preceding verified backup increment.
Run affected binary/network integration tests, strict lint, format and builds on
stable/minimum Rust. Do not present unchanged core/fuzz suites as newly executed.
Docker is unavailable locally; container v2 recovery and upgrade acceptance remain
open. See the [contract](../v2-server-startup.md).
