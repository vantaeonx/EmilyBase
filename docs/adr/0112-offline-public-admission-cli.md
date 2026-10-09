# ADR0112: explicit offline public admission CLI

Status: accepted for explicit offline operator administration after these checks.

## Context

The private v5 catalog and native admitted user gateway are verified. An operator
still needs a practical way to select a project's admission state without writing
a custom Rust program or placing a service key in a command argument. Catalog
migration, public admission and session revocation must remain distinct actions.

## Decision

Add account-admission for an existing stopped private root, explicit project ID
and shared private --key-file. Subcommands are enable-catalog, status, open and
close. The first explicitly migrates v4 to closed v5; the original policy catalog
must already exist. Other commands never migrate. A repeat enable-catalog on v5
preserves its current state, including an already-open flag.

Open/close require --expected with canonical decimal u64 digits. Parse before any
key-file loading or root ownership. Preserve exact revision/previous strings in
JSON output; never round through floating point or signed integer conversion.
The original retained-root current-key authorization, actual selected project,
exclusive private owner and original admission CAS decide every operation.
An old service key or another project's key cannot read status or change a flag.

Reuse the bounded original0600/0400 single-link regular-file credential loader,
no-follow descriptor checks and zeroized secret owner. No password, token, key
contents, filesystem path or user rows are returned by this command. Output is
only current private version and enabled/revision/previous metadata. It is never
request authority. No stdin wait, caller-selected clock or implicit reset exists.
The existing root lock refuses while the same server/root is active.

Reuse original native CAS without automatic conflict retry. Identical operations
accept only the recorded predecessor/current revision and perform no write.
A stale open after an intervening close refuses. Unrelated private commits do not
replace the admission receipt. The native reserved WAL maximum still refuses.

Closing suspends native public operations without revoking session families. An
intentional reopen may resume a current unexpired token; use explicit account or
session revocation when intended. Verified restore closes an enabled copy while
resetting incarnation/time. Reopening it does not revive source tokens: fresh
login is required. Data and policies survive; the source can remain active.

The durable operation finishes before stdout. A failed terminal/pipe write cannot
prove rollback. Return a static inspection-required diagnostic and leave the
operator to inspect status or submit only a documented exact retry. A controlled
real /dev/full test must observe the committed state and readonly exact retry.
This is not a cross-filesystem transaction or a new durability protocol.

## Verification plan

Actual binary tests cover explicit v3 refusal/v4 migration, closed/default and
readonly status/migration/retry, current native owned row access, suspend/resume,
ABA conflict, current/rotated/cross-project service key refusal, active-owner
refusal, malformed canonical digits/project names before root selection, original
private-file constraints including FIFOs, terminal write failure, and nonempty
verified multi-project clone with fresh login. An independent generated CAS model
compares current/predecessor revisions, exact no-op WALs and unrelated histories
on both original WAL formats. Unit/property checks preserve the full u64 domain.
Rerun original CLI command, policy, user and key-rotation suites on stable/minimum
Rust, strict workspace/fuzz formatting/linting and minimum build/compilation.

No public HTTP, new decoder/format, sanitizer campaign, signup, role, user SDK or
dashboard is introduced. Offline activation does not imply network or production
readiness; the corresponding acceptance gates remain open.


## Final verification

The complete CLI crate passes97 checks on each of Rust1.99 and1.89, across26
test summaries. Nine regular cases are new: seven actual binary scenarios and
two unit/property checks. Sixteen generated actual CLI CAS sequences and32
generated unsigned values per toolchain cover the new control boundary. Two real
stdout failures per toolchain observe committed state and readonly exact retry.
Both original WAL formats, nonempty two-project verified copy, current users,
service-key rotation and private file/owner refusals are covered. No new process
kill, parser sanitizer campaign or power-loss proof is claimed.

Strict workspace/fuzz formatting and Clippy, minimum workspace build and all-target
fuzz compilation pass on495 frozen source/dependency/API hashes. Added781 Rust
lines; total105215 physical source lines, Rust100928 (97080 effective), SDK2502
and Python1785. See
[source-bound evidence](../measurements/2026-10-09-offline-public-admission-cli/verification.json).
Public HTTP/signup/roles/user SDK/dashboard, load/upgrade/resources and independent
security/production acceptance gates remain open.
