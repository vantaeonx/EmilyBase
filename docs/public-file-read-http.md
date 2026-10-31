# Admitted current-user file reads over HTTP

[ADR0178](adr/0178-public-file-read-http.md) connects the verified native user-file
read gateway to account-root HTTP. Explicit native v2 startup, private v6 file
policy, open public admission and a genuine current user access session are
required. There is no implicit file creation, migration or policy installation.
Trusted native administration currently configures file policy and physical quota.

| POST path below /v1/projects/{id} | Exact JSON body | Successful response |
| --- | --- | --- |
| /user/files/list | {} | {"files":[metadata,...]} |
| /user/files/info | {"id":"32 lowercase hex digits"} | {"file":metadata-or-null} |
| /user/files/read | {"id":"32 lowercase hex digits","maximum_bytes":8388608} | complete binary bytes |

Each call requires exactly one Authorization: Bearer current user access token.
Service/master keys and refresh credentials cannot substitute. Query parameters
and X-EmilyBase-Access are not credential sources. Static route/method/status/
timing logging excludes IDs, bodies, tokens, display names and query values.
All responses, including failures, carry Cache-Control:no-store and Pragma:no-cache.

## Grammar and responses

Requests must be object-only application/json, at most4096 actual bytes and
complete within five seconds. Arrays, unknown or duplicate fields, noncanonical
IDs, floating/string/negative maximums and values over8388608 are rejected.
maximum_bytes zero permits only a visible empty file; the bound does not grant
read permission. Grammar validation precedes service-clock observation and never
reads a file, authenticates a session or installs metadata.

Metadata contains exactly id, name, payload_bytes and revision. Revision is a
canonical decimal string so JavaScript does not round a u64. The other numeric
field is at most8MiB. The response contains no physical object ID/path, owner or
digest. Lists include only current policy-visible references, bounded by the
native128-object cap. Missing and hidden info both return {"file":null}; neither
physical orphans nor another user's hidden reference become downloads.

A binary response is application/octet-stream with X-Content-Type-Options:nosniff.
No caller-controlled filename is inserted into headers. The native method copies
and verifies the complete bounded payload under its original owners and current
proof before HTTP prepares any success bytes. The implementation returns a buffer,
not a partially checked streaming cursor. Range is unsupported and ignored: a
successful request returns the complete200 payload, without Content-Range.

| Status | Static code / meaning |
| --- | --- |
| 400 | file_request_rejected: invalid strict request |
| 401 | access_denied: missing/malformed/duplicate credentials, closed preadmission or invalid current session |
| 403 | file_read_rejected: missing/hidden payload, unavailable installed policy or admission closed after a wait |
| 408 | body_timeout |
| 413 | body_limit, or file_read_limit when a visible payload exceeds maximum_bytes |
| 415 | json_required, including ambiguous duplicate Content-Type |
| 429 | original peer/project rate budget exhausted |
| 500 | existing worker/peer failure |
| 503 | file_read_unavailable or existing static storage/clock/worker refusal |

Bodies never include Rust errors, credential data or a payload prefix on failure.
Header-shape and memory-only admission checks precede body consumption. Actual
current authentication/policy runs in the blocking native frame after body/root
waits. Closing admission, revoking/refreshing a session, disabling/re-enabling a
user, changing password epoch or replacing policy during those waits takes effect
on the pending operation. The original four permits and30/minute project budget
are shared with existing admitted user calls, not reset per file route.

## Verification and limits

The [source-bound verification](measurements/2026-10-31-public-file-read-http/verification.json)
records exact commands and frozen inputs. Tests use genuine original-engine v2
roots, distinct users/projects and both WAL versions. They cover current policy,
hidden/missing equivalence, strict bodies/headers, body timeout/cancellation,
worker/project budgets, changed credentials during body/root waits, actual source
substitution/corruption and independent restore with revoked copy sessions. Raw
TCP tests verify arbitrary binary bytes, empty and128KiB files, denial and graceful
restart while preserving exact native/private histories at fixed service time.
The pure grammar also receives an AddressSanitizer fuzz campaign; this is bounded
verification, not a complete security audit.

Authentication can independently advance the private clock on forward service
time, including a later denied read. Old metadata/bytes are not future permission.
Source checks are finite observations, not a filesystem lease against a malicious
host. Shared worker permits end after response preparation; a slow client's queued
response buffer is not a whole-process memory reservation. Aggregate admission,
container-v2/power-loss drills and every unmet release/security gate remain open.
No actual user data is used. HTTP mutations, SDK file methods, signed URLs, roles,
realtime, dashboard and Kotlin remain open.
