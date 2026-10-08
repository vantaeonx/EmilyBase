# ADR0103: Service-key policy administration over a held real table context

Status: accepted for service-key administration after the checks below; no end-user data route.

Add explicit enable/list/install operations only to the retained account-root
service and its private HTTP router. Current project service keys are required;
master, sibling and user access/refresh credentials grant no policy administration.
No legacy registry route or implicit private migration is added. Enabling v4 does
not observe time or change session incarnation. No caller-provided project/table
ID/schema is accepted as target authority.

The synchronous root derives table ID and complete schema from the real public
snapshot while holding the existing authorized data gate and original database
owner. It holds them through the private policy commit. All complete inventory,
expected revision, exact current/predecessor retry and atomic fragment replacement
rules remain those of ADR0102. Public rows/WAL remain unchanged by policy work.
Dropped/recreated tables receive fresh identities and need a new explicit install;
old identities remain bounded catalog entries until a separate retirement design.

HTTP install accepts strict table/expected/document fields. Expected is a canonical
full-u64 decimal string; document is an exact JSON-definition string bounded at
16KiB before policy decoding. Overall request/response is bounded at64KiB. Enable
accepts only an empty object; list returns sorted metadata, never definitions.
All table/revision/predecessor u64 values use decimal strings and digests use
lowercase hex. The current key/private roster is rechecked after waiting for body
completion and before decoding. Existing four-worker, peer/private attempt bounds,
timeout, no-cache responses and secret-free logging apply.

Use explicit static conflict codes for disabled catalog, revision conflict and
capacity. Invalid requests are400; semantic corruption is503. Private storage or
post-commit response uncertainty is503 policy_outcome_requires_inspection, never
an inferred rollback or automatic retry. Existing account failure contracts keep
their default mapper; policy operations use a dedicated mapper at the same blocking
owner boundary. End-user tokens still cannot call SQL, rows or policy routes.

Acceptance requires strict/duplicate/size/full-u64 tests, concurrent exact/stale
CAS behavior, current-key rotation during both body waits, sibling/private/public
history preservation, actual owner retention and recreated-ID tests, native
received-HTTP-ACK kill/reopen/exact-retry for enable/first/replace on WAL1/2, verified
common-root clone with policy preservation and old-session denial, private-router
regression on stable/minimum Rust and strict lint/fuzz checks. The original auth
catalog's staged/received-ACK crash matrix remains in ADR0102. No platform stage
or user enforcement gate is closed by policy administration.

Frozen-source acceptance passes44 cases on each Rust toolchain:32 private HTTP/
grammar/regression,11 native account network and one real held-context test.
Nine cases are new. Native policy flow receives six write ACKs before forced
kills and leaves two write responses unread before separate inspection, kill and
explicit exact retry per toolchain, over WAL1/2. Verified root clones preserve
policies and revoke source sessions. Strict formatting/Clippy, minimum workspace/
fuzz compilation and5,615,985 ASan request runs pass without findings. OpenAPI
local references,33 operations and required path parameters are checked.
See [source-bound evidence](../measurements/2026-10-09-policy-admin-http/verification.json).
These results accept administration only; broader user enforcement remains open.
