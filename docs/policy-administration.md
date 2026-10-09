# Service-key policy administration

Only the retained account-root mode exposes these endpoints. A current project
service key is required in the Bearer header. Master keys, sibling keys and user
access/refresh tokens cannot administer policies. This does not grant users a
SQL/row route or implement roles. Legacy registry mode has no policy endpoints.

| Method and path | Request | Result |
| --- | --- | --- |
| POST /v1/projects/{id}/auth/policies/enable | empty object | private_version4 after explicit atomic migration |
| GET /v1/projects/{id}/auth/policies | no body required | complete sorted policy receipt inventory |
| POST /v1/projects/{id}/auth/policies/install | table, expected, document | committed or exact existing receipt |

Enable is explicit and idempotent; no clock observation, session reset or default
root upgrade occurs. Existing v3 sessions remain valid. Before enable, list/install
return409 policy_catalog_disabled. See [v4 compatibility and persistence](policy-catalog.md).

Install accepts exactly these fields:

```json
{
  "table": "items",
  "expected": "0",
  "document": "{\"version\":1,\"select\":{\"kind\":\"deny\"},\"insert\":{\"kind\":\"deny\"},\"update_using\":{\"kind\":\"deny\"},\"update_check\":{\"kind\":\"deny\"},\"delete\":{\"kind\":\"deny\"}}"
}
```

`document` is a JSON string containing the exact original policy definition bytes
once decoded as UTF-8. It is not an object reserialized by the server. Whitespace
inside that string matters to the policy checksum. The definition is limited to
16,384 UTF-8 bytes; the entire JSON request and response are limited to65,536 bytes.
Unknown/duplicate fields, client-supplied project/ID/schema/time assertions,
noncanonical expected digits and unsupported definition grammar refuse.

Expected is0 for a missing policy, otherwise its current private commit revision.
An exact currently installed definition/schema with current or recorded predecessor
expectation returns the same receipt without a write. Changed stale content returns
409 policy_revision_conflict. Receipt table/revision/previous use full decimal-string
u64 values, and sha256 is64 lowercase hex characters. JavaScript numbers are not
accepted for these fields. The receipt contains metadata, never a definition.
There is no automatic retry and no operation-history audit beyond the current
receipt/predecessor. The separate [record codec](policy-records.md) preserves exact
bytes and the original engine assigns actual private committed revisions.

The root resolves the named table in the real public database, derives its stable
ID and full schema, and holds the authorized data gate/database owner through the
private commit. A request cannot choose a fabricated target schema/ID. Public
rows and WAL remain unchanged. Dropping/recreating a table creates a new identity;
it requires a new install with expectation0. Obsolete policies remain bounded
entries; there is no unsafe delete/reuse or capacity-reclaim endpoint.

Key and private roster are checked before admission, then checked again after body
completion before decoding. Existing four-worker admission, peer/private attempt
limits and five-second body deadline apply. Responses are no-store/no-cache and
logs contain route/status only, never keys/tokens/passwords/policy definitions.
A future SDK/dashboard can call this explicit service interface, but neither is
added by this increment.

Invalid request/definition/table lookup is400 policy_rejected or table_rejected.
Disabled catalog, stale revision and full catalog have separate409 static codes.
Corrupt policy inventory is503 policy_catalog_invalid. Private storage or post-write
response uncertainty is503 policy_outcome_requires_inspection; worker/admission/
filesystem failures can also return503. A disconnected write client must not assume
rollback. Reopen/inspect and explicitly retry only the exact original definition
and expected revision when its current/predecessor contract applies. Changed later
content correctly refuses a stale retry rather than overwriting it.

Synchronous trusted code can also call AccountRoot.enable_row_policy_catalog,
row_policy_receipts and install_row_policy. These keep the current service-key and
held real-table boundary. A separate synchronous
[user-row gateway](user-row-enforcement.md) requires both the current service key
and a current user access token to apply installed rules under the original owners.
The [backend HTTP adapter](user-row-http.md) uses that same two-credential contract.
It grants no detached private handle or user-only SQL/HTTP authority. See
[ADR0103](adr/0103-service-key-policy-administration.md) and [OpenAPI](openapi.json).
Native filtered pages are also available through the trusted gateway. Public
admission, retirement, security, upgrade, resource and
production gates remain open.


For an existing stopped root, the Rust [offline policy CLI](policy-cli.md) uses
these same current-key and real-table methods. It reads credentials from a bounded
private file and the exact definition from stdin; it neither creates a root nor
grants a public user route.
