# Typed project row API

Experimental trusted backend interface in both server data modes. Every route
requires the selected project's service key. Do not place that key in browser or
mobile applications. Master keys, access tokens and refresh tokens grant no row
permission. Responses/refusals disable caches. This uses the original synchronous
storage engine directly, without SQL interpolation.

All routes are POST under `/v1/projects/{id}/tables/rows/`:

| Suffix | Strict JSON fields | Successful200 response |
| --- | --- | --- |
| `get` | `table`, `key` | `{ "row": [...] }`, or `{ "row": null }` if absent |
| `page` | `table`, `limit`, optional `after` | `{ "rows": [...], "next": key-or-null }` |
| `insert` | `table`, `row` | `{ "key": key, "transaction": "3" }` |
| `update` | `table`, `key`, `row` | same mutation acknowledgment |
| `delete` | `table`, `key` | same mutation acknowledgment |

`row` is an array in schema column order, with exactly its column count. Unknown
and duplicate fields refuse. Each value has a `type` tag:

| Type | `value` |
| --- | --- |
| `null` | omitted or null |
| `boolean` | JSON boolean |
| `integer` | canonical decimal string in signed64-bit range, e.g. `"9223372036854775807"` |
| `float_bits` | exactly16 lowercase hexadecimal IEEE754 binary64 bit digits; finite only |
| `text` | UTF-8 string, maximum3072 bytes |
| `bytes` | array of0..255 integers, maximum3072 entries |

A primary `key` uses only integer or text with the same representation. Integer
strings reject leading zeros, plus signs, whitespace and negative zero. Float bits
preserve signed zero and every finite bit pattern. The logical
[table exchange](table-transfer.md) uses numeric JSON integers; do not substitute
its document directly for this row protocol. Transaction IDs are decimal strings.

Example insert body for an integer/text schema:

```json
{
  "table": "items",
  "row": [
    { "type": "integer", "value": "9223372036854775807" },
    { "type": "text", "value": "synthetic item" }
  ]
}
```

Update replaces the complete existing row and forbids changing its primary key.
Delete requires an existing row. Insert refuses a duplicate key. Each mutation is
one original durable WAL transaction; its response follows commit/fsync. Failed
validation does not append a mutation. An interrupted/uncertain response requires
inspecting current state before retrying; no idempotency key is implemented.

Page `limit` must be1..128; `after` may be absent/null or a typed exclusive key.
Rows follow ascending integer or UTF-8 byte key order. If more rows remain, `next`
is the last returned key; otherwise null. A deleted continuation key still works.
Each page reads current committed state; mutations between requests can change
what the caller sees. There is no count or cross-request snapshot.

Body and serialized response each cap at65,536 bytes, with a5-second body deadline
and four shared workers. A page that exceeds the byte cap refuses completely;
reduce `limit` and retry. Single rows remain bounded by64 columns,3072-byte values
and the original4000-byte encoded-record limit. Ordinary typed refusals use
`table_rejected`400; body/admission/storage/uncertain-transaction errors retain
existing static codes. [OpenAPI](openapi.json) describes the wire.

Private-root credentials are rechecked after body waits. Legacy already-admitted
request authority follows SQL policy. Public row work neither observes private
session time nor writes private account history. Row-level policies, public user
authority, global resource budgets and production acceptance remain open.
[ADR0092](adr/0092-bounded-project-row-api.md) records the decision.
