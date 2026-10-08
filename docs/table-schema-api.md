# Project table schema API

Experimental trusted backend interface in either server data mode. Every route
requires `Authorization: Bearer <project-service-key>` for the URL project.
Keep this key on the backend. Master keys, access tokens and refresh tokens do
not grant table authority. Responses and refusals disable caching.

| Method and project-relative path | JSON input | Successful JSON output |
| --- | --- | --- |
| GET `/v1/projects/{id}/tables` | none | `{ "tables": [ { "id": "1", "name": "items", "columns": 2, "primary_key": "id" } ] }` |
| POST `/v1/projects/{id}/tables/schema` | `{ "table": "items" }` | complete typed schema |
| POST `/v1/projects/{id}/tables/create` | complete typed schema | `{ "table": { "id": "1", "name": "items", "columns": 2, "primary_key": "id" }, "transaction": "1" }` |
| POST `/v1/projects/{id}/tables/drop` | `{ "table": "items" }` | `{ "transaction": "2" }` |

All successful operations return200. Example schema:

```json
{
  "name": "items",
  "columns": [
    { "name": "id", "data_type": "int", "nullable": false },
    { "name": "title", "data_type": "text", "nullable": true }
  ],
  "primary_key": 0
}
```

The primary key is a zero-based column index and must be a non-null int or text.
Supported column types are boolean, int, float, text and bytes. Names follow the
catalog ASCII identifier rules; maximum63 bytes. Schemas have1..64 columns and
must also fit the original encoded schema. Unknown/duplicate fields and invalid
schemas refuse. Inventory lists up to128 live tables in ascending ID order.
IDs and transaction IDs are decimal strings; never convert them through a
floating-point client number if exact precision matters.

Create refuses an existing name. Drop removes schema and rows in one durable
transaction; a later recreation is empty and receives a new ID. There is no
implicit overwrite, cascade policy, schema migration or destructive confirmation
UI. A client with the trusted service key already has this destructive authority.
The reply follows the original commit/fsync acknowledgment. An interrupted or
uncertain write requires inspecting current state before retrying; this protocol
has no idempotency key.

Input/output max65,536 bytes; body deadline5 seconds; four shared admitted workers.
Metadata reads do not observe private session time or append either WAL. Public
writes do not append private account history. Private-root mode rechecks the
current project key after body waits; legacy already-admitted request policy
matches SQL. Invalid requests return static `table_rejected`400; authorization,
body, admission and storage errors use the existing static codes described in
[OpenAPI](openapi.json). No response or log echoes rejected input.

[ADR0091](adr/0091-project-table-schema-api.md) records the design.
User roles/RLS, public client authority, whole-process memory/connection admission
and production acceptance remain open. Use synthetic data only.
