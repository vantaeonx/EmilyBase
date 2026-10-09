# Policy-enforced rows over trusted backend HTTP

Retained account-root mode exposes three POST routes. Each requires a current
project service key in `Authorization: Bearer ...` and exactly one
`X-EmilyBase-Access` header containing the current user access token. Service keys
belong only in trusted backend code. Browser/mobile clients must not receive them.
This is a backend adapter, not public user-only admission or a role system. Legacy
registry mode exposes none of these routes; user tokens alone grant no SQL or data
route. Existing service SQL/rows retain their separate trusted authority.

| Path | Strict request | Result |
| --- | --- | --- |
| /v1/projects/{id}/auth/rows/get | table, key | row or null, masking absence and SELECT denial |
| /v1/projects/{id}/auth/rows/page | table, after, limit | permitted rows and visible next key, or null |
| /v1/projects/{id}/auth/rows/write | table, operations | changed operation count and actual transaction string |

Requests reuse [typed row transport](row-api.md): integer values are exact canonical
signed decimal strings, finite floats use bit text, and bytes use bounded arrays.
Page limits are 1..128 and write packets contain 1..256 ordered insert/update/delete
operations on one table. No SQL interpolation, client time/project/schema/identity
assertion or unknown/duplicate field is accepted. Write grammar is the service
batch grammar, but execution always enters the [owned user-policy gateway](user-row-enforcement.md).
Single service insert/update/delete routes are not forwarded by this adapter.

Example body for a point read:

```json
{"table":"items","key":{"type":"integer","value":"9223372036854775807"}}
```

The access header is separately bounded to the existing 102-byte token format;
missing, duplicated, non-ASCII, malformed/purpose-mismatched or revoked tokens refuse.
The owned header copy is zeroized on drop, without a whole-heap erasure guarantee.
Passwords, tokens, row bodies and policy definitions are never logged. Responses
carry no-store/no-cache. Existing peer/project attempt limits, four shared workers,
five-second body deadline and 65,536-byte complete request/response caps apply.
Private project attempts are currently limited to 30 per minute, including reads.

After waiting for the body the root rechecks current service key/private roster
before decoding. It derives actual table identity/schema under the original public
owner and verifies current private session/policy/time under its retained owner.
Both remain held through the operation. Key rotation, policy replacement or session
revocation during the wait applies before public work. Schema-specific key types
are checked before physical lookup, including on empty tables; physical storage
errors are not converted into client input errors.

Get/filtered pages preserve the native hidden-row and current-state semantics.
A continuation reveals only the last returned visible key when another permitted
row exists; it is not a stable multi-request snapshot. Complete JSON can exceed
64KiB even for a native page within its count bound; the adapter then returns an
error without a partial page. Explicitly request a smaller limit for that read.

Write checks actual staged old/new rows and commits once through original WAL/fsync.
Any policy/typed/late-constraint refusal discards the complete packet. A successful
response returns the actual transaction as a full u64 decimal string. Private
forward time observation is separately durable and can survive public rejection.
There is no automatic retry or idempotency receipt. A disconnected client must
inspect current data; an explicit duplicate insert can refuse after the first
request committed. Do not infer rollback from a lost or error response.

Static errors include 400 user_row_rejected/table_rejected, 401 access_denied,
403 user_row_rejected, 409 policy_catalog_disabled, and 503
user_row_outcome_requires_inspection for storage/response uncertainty. Invalid
stored policy binding/catalog, trusted clock or admission failures also return
static unavailable codes. Missing policies never grant access.

Router tests cover both WALs, body-wait revocation/rotation, strict headers/input,
limits, unchanged sibling/equal-clock private histories, large response failure,
private attempt bounds and legacy absence. Real TCP tests receive write ACKs before
forced kills, also leave write responses unread before independent inspection and
kill, verify atomic reopen, duplicate refusal and common-root clone with fresh
sessions. See [OpenAPI](openapi.json),
[ADR0106](adr/0106-trusted-backend-user-row-http.md) and
[verification](measurements/2026-10-09-trusted-user-row-http/verification.json).
Public user-only admission, roles, SDK adapters and broader security/load/upgrade/
resource/production gates remain open. Use synthetic data only.


The subsequent [public row HTTP adapter](public-row-http.md) consumes the same
typed grammar and executor using only current admitted user access. Existing
trusted service+user routes retain their credentials; no service-key fallback
is introduced into the public namespace.
