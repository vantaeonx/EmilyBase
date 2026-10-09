# ADR0108: offline private user provisioning and metadata

Status: accepted for trusted offline user administration after these checks.

## Context

The original account root already exposes service-key-only user creation, current
metadata pages and credential-epoch disable/enable transitions. Local operators
need these actions without putting passwords in command history/process arguments
or requiring a running HTTP server. Existing APIs must keep their private owner,
current-key, KDF and session-revocation behavior.

## Decision

Add Rust `account-user ROOT PROJECT --key-file FILE` commands create, list, disable
and enable. Use the shared bounded private API-key file loader. Open only existing
roots and retain the original exclusive owners. No key, password, token, verifier
or session is issued by these commands. Metadata intentionally includes user ID,
canonical login, exact decimal-string epoch and disabled state for trusted operators.
List follows the original exclusive-login cursor and 1..128 row bound.

Create accepts only redirected stdin, limited to1..1024 raw password bytes. A real
terminal refuses before reading to avoid visible terminal echo. Preserve every
byte including final LF; do not trim, normalize, truncate or require UTF-8. The
native password engine is byte-oriented. Callers using the JSON HTTP sign-in API
must provision passwords representable by that API's UTF-8 string contract.
Validate bounded stdin before acquiring a root owner. A waiting pipe holds no
database lock; current service authorization occurs inside the native root after
input completes. Owned buffers zeroize on drop; file/pipe/OS/operator copies are
not erased by this guarantee. Nested read errors remain static and omit contents.

Use the existing one-operation PasswordPool and Argon2id policy. Create is trusted
provisioning, not anonymous registration or user role authority. Disable/enable
use original credential epoch transitions: repeated unchanged state is a no-op;
re-enabling sign-in never resurrects old access/refresh credentials. Neither list
nor provisioning observes or resets trusted session time. No private version or
stored format is introduced; explicit v4 policy migration remains independent.

## Consequences

The server must stop before offline use. stdin has no deadline and needs EOF for
accepted passwords; oversized still-open streams fail after limit+1 bytes. A key
rotated during the wait rejects the previously loaded value. Incomplete-input
process termination happens before acquisition and changes no WAL. Concurrent
provisions for the same login yield one successful account, with either busy or
already-exists refusal for the competitor. No password replacement or auto-retry
occurs. Lost stdout after commit can be ambiguous: inspect metadata first; existing
login refusal does not prove a guessed password or authorize a reset.

Output is capped at65,536 bytes. Metadata is privileged operator data, never a
session or authorization receipt. Key files remain separate plaintext operator
configuration outside root bundles. Verified root restore preserves verifiers,
epochs, disabled state and policies while resetting session incarnation.

## Verification

Ten real executable scenarios exercise WAL1/2 and private v3/v4 provisioning,
exact password/login behavior, ordered metadata models, disabled epochs/no-ops,
current-key changes during observed pipe waits, exclusive owners, same-login
competing processes, maximum binary passwords, incomplete-input kills and verified
nonempty clones. A real Linux pseudoterminal verifies visible-input refusal without
entering a password; its test fixture uses Python3, not a runtime dependency.
Four pure cases include128 generated raw password streams, limit+1 reader accounting,
static read failures and full-u64 metadata output. Final frozen-source checks pass104 cases per Rust1.99/1.89: all82 CLI cases
across24 binaries,17 original password cases and five private pagination cases.
Fourteen regular cases are new. Both WALs have an incomplete-password-input kill
and a same-login competing-process pair per toolchain. Strict workspace/fuzz
formatting/Clippy, minimum workspace build and all-target fuzz compilation pass
on484 frozen source/dependency/protocol files. See
[source-bound evidence](../measurements/2026-10-09-offline-user-cli/verification.json).

No fresh sanitizer campaign is claimed: no SQL/JSON/policy or stored-format parser
changes. Existing fuzz targets are compiled/linted; raw input has independent
bounded/property/process checks. Broader security/load/upgrade/resources, public
user admission, roles and production gates remain open.
