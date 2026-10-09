# ADR0107: offline current-service-key policy administration

Status: accepted for trusted offline administration after the checks below.

## Context

The original root service already owns public and private stores and can enable,
list and install policies under the current project service key. Its HTTP interface
is useful to a trusted backend; local operators also need an explicit offline
workflow that preserves the same ownership, exact revision and private-key rules.
Passing a key as a command argument would expose it to command history/process
inspection. Duplicating the existing bounded secret loader would risk drift.

## Decision

Add the Rust `account-policy ROOT PROJECT --key-file FILE` group with enable,
list and install operations. Install reads the exact original definition from stdin,
limited to 16,384 bytes. Expected revision accepts only canonical u64 digits; no
normalization or default expectation is permitted. Finish bounded input validation
before acquiring the retained root owner. Root open never creates missing paths;
current service authorization occurs afterward, inside the existing native method.

Move the original private-file reader into auth::key_file and reuse it from both
server configuration and CLI. Keep the server's environment/file exclusivity and
static diagnostics. Read at most 66 bytes from a regular singly linked 0400/0600
file with NOFOLLOW/NONBLOCK/CLOEXEC; compare retained/selected metadata before and
after reading. Accept only 64 lowercase hexadecimal characters and optional final
LF. Typed static errors omit paths/content. Owned buffers are zeroized; the secret
file and external copies remain plaintext. A test verifies loaded-secret Debug
formatting does not reveal the key. Trusted operators control parent directories.

Enable remains explicit private v3-to-v4 migration, preserving clock/sessions.
Install derives the actual public table ID/schema under the original held owner
and performs the existing atomic private commit. Exact current/predecessor retries
are no-ops; changed stale definitions fail. CLI output contains bounded receipt
metadata with table/revision/previous as decimal strings and a SHA-256 hex digest.
Neither policy definitions nor key/password/token values are printed.

## Consequences

The server must be stopped; a live root refuses as busy. Waiting on operator stdin
holds no database owner and has no HTTP deadline. A copied key does not bypass a
later rotation: the current root checks it after input completes. Oversized input
fails after limit+1 bytes even if the writer remains open. Incomplete-input process
termination performs no commit. A successful write followed by stdout/flush failure
can already be durable: inspect and deliberately retry only the exact unchanged
definition/expectation; there is no automatic retry or rollback promise.

No new database, policy, token, archive or root format is selected. No role, public
user endpoint, dashboard, key provisioning or implicit migration is added. Root
initialization still prints no usable key. This interface is trusted local operator
authority, not a sandbox against that operator. Production gates remain open.

## Verification

Eight real CLI cases exercise both original WALs, current-key changes during an
observed pipe wait, exclusive-owner refusal, bounded/exact stdin, incomplete-input
kill, recreate identities, CAS retries and verified nonempty root restore with
revoked old sessions. Pure tests cover exact u64 output/input, bounded reader calls
and redacted read errors. Shared key-file tests retain deterministic replacement,
permissions, links, Unicode, malformed input and generated keys. Existing server
file-startup tests rerun to check both data modes after the extraction.

Frozen-source final checks pass78 cases on each Rust1.99/1.89: all68 CLI cases
across23 test binaries, six shared key-file cases, one server config case and three
real server file-startup cases. Twelve regular cases are new. Both WAL versions
receive two incomplete-input process kills per toolchain; these are before root
acquisition, not new kills during a WAL write. Shared file properties generate16
keys per toolchain. Strict workspace/fuzz formatting/Clippy, minimum workspace build
and all-target fuzz compilation pass on481 source/dependency/protocol hashes.
See [source-bound evidence](../measurements/2026-10-09-offline-policy-cli/verification.json).
No fresh sanitizer campaign is claimed: the policy grammar and stored codecs are
unchanged and the reused filesystem loader is exercised by unit/property/process
cases, not a filesystem fuzzer. Full workspace runtime, local Docker, independent
security and load/upgrade/resource audits are separate checks.
