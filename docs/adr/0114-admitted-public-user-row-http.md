# ADR0114: admitted public typed row HTTP

Status: accepted for admitted typed user row HTTP after these checks.

## Context

Public sessions and the native policy-enforced user row gateway are verified.
Clients still need service credentials for the existing trusted row HTTP adapter.
Expose only the bounded native user operations through the current admitted user
scope, preserving original authority, storage and serialization protocols.

## Decision

Add three POST /v1/projects/{id}/user/rows routes: get, page and write. Extend the
separate public middleware's explicit access-only route set. Require one current
Bearer access credential; no service/master key or X-EmilyBase-Access fallback.
Keep UserScope internal, expose only its project/access readers to the sibling
adapter, and retain the original permit/root owner. Share current flag preadmission,
real-peer/project rate budgets, static route logs and no-store response middleware.

Read the original64KiB/five-second body and run the shared blocking owner. Recheck
the current flag after body/root waits before decode/server clock. Decode only the
original lossless tagged get/page/packet grammar; pass it to public_user_table.
That original native method authenticates before public data/table metadata,
derives actual current table identity/schema/policy proof under both owners and
consumes it through the original executor/commit. Reuse the original row response
serializer and typed failure mapping. No arbitrary SQL, DDL, unfiltered handle,
user-selected principal or migration-ledger access is exposed.

Own/hidden reads and visible keyset pages retain original semantics. Missing policy
denies. Every staged packet obeys current USING/CHECK, actual primary uniqueness,
immutable primary keys and complete rollback on a late conflict/policy refusal.
Closing admission, disabling/revoking the user or changing a policy during body
wait applies before the final public operation. Service-key rotation remains
independent of current user sessions. Shared limits and storage uncertainty are
unchanged; never automatically retry a lost write result.

## Reproduced correction

An independent test exercises outer positional get/page/batch arrays through both
original service and user grammar validators. All six paths accepted undocumented
arrays through Serde before correction. Require an outer object in the shared row
parser after its byte bound, while preserving inner typed row/value arrays.
The same decoder remains used by trusted/public adapters and pure fuzz validators.
The fuzz target independently asserts object shape and byte bounds for successful
service/user parses. No persisted or tagged value format changes.

## Verification plan and boundaries

Stable/minimum Rust: new actual router scenarios cover owned CRUD/hidden pages,
complete late rollback and duplicate/foreign writes, access/purpose/project and
authentication-before-data metadata, ledger exclusion, delayed policy/flag/epoch,
exact signed i64 wire, strict object/duplicate/header/body/packet bounds and one
concurrent primary-key winner. Actual TCP checks received packets before forced
stops, an unread write result whose complete commit is independently observed,
no blind retry, both WALs, and nonempty verified copy requiring reopen/fresh login
while the source remains active. Recheck original HTTP/native/row grammar/network
suites, strict lint/format/minimum compilation, original row request ASan target
and exact OpenAPI references/security.

Body grammar validation precedes native access verification, but never opens data
or resolves table metadata. Invalid current credentials can observe original clock
progress; private time and public row commits are separate transactions. Hidden
reads do not reveal row contents; write refusal/global primary uniqueness do not
provide a general noninterference guarantee for key occupancy. These are explicit
existing limits, not new production claims.

No signup, role, user SQL, cross-origin/cookie contract, user SDK/dashboard or
production acceptance is enabled. Process kills do not prove power-loss safety;
pure grammar ASan does not audit the complete network/authentication/policy flow.


## Final verification

Rust1.99/1.89 pass101 checks each: account HTTP54, original row grammar16,
native public9, actual account network14, original HTTP7 and documentation1.
One original process helper remains intentionally ignored. Seven regular cases
are new. Six new forced stops per compiler cover four received write results and
two unread results after independent complete-commit observation on both WALs.
Nonempty verified copy requires reopen/fresh login while source remains active.
Existing native24 generated models/four packet kills are rerun.

Final row_requests ASan executes3516827 inputs in46 seconds, RSS351MiB
under512, max input65538/request65536 bytes,6309 initial seeds including15 new
object/boundary seeds, no findings. This is pure grammar/mapping, not a complete
HTTP/auth/policy audit. Workspace/fuzz formatting/strict linting, minimum workspace
build and all-target fuzz compilation pass on504 frozen source/dependency/API
hashes. OpenAPI43 operations/478 local refs resolve. Added891 Rust lines; total107327
source, Rust103040 (99148 effective), SDK2502 and Python1785. See
[source-bound evidence](../measurements/2026-10-09-public-row-http/verification.json).
Signup/roles, user SDK/dashboard/browser integration, load/upgrade/resources and
independent security/production acceptance remain open.
