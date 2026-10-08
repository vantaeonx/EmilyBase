# EmilyBase TypeScript client

Experimental project-scoped ESM SDK. Apache-2.0. No runtime dependency. The package
is local/private until a separate release; it is not published on npm. Node 22 is
the tested environment. Build from the repository root:

```sh
npm ci --ignore-scripts --prefix sdks/typescript
npm test --prefix sdks/typescript
cargo build --locked -p emilybase-server
npm run test:integration --prefix sdks/typescript
```

Import `EmilyBaseClient`, `EmilyBaseError` and TypeScript types from the built
`sdks/typescript/dist/index.js` or a local package link:

```ts
const client = new EmilyBaseClient({
  url: "http://127.0.0.1:7000",
  project: projectId,
  apiKey: privatelySuppliedProjectKey,
});
const report = await client.sql(
  "SELECT id,title FROM items WHERE id=$1 LIMIT 10",
  [{ type: "integer", value: 7 }],
);
const plan = await client.explain("SELECT * FROM items WHERE id=7");
const status = await client.status();
client.setKey(privatelySuppliedReplacementKey);
client.close();
```

Project creation/rotation are administrator operations through the documented
[HTTP API](../../docs/server.md). The SDK exposes project SQL, explain, status and typed row operations. Project keys currently grant full access to one project;
user/row policies do not exist. Use synthetic data and keep credentials private.
No default credentials, persistence, logging, retries or administrator client.

## Behavior and limits

Parameters remain separate from SQL. Values use tagged `null`, `boolean`,
`integer`, `float`, `text` or `bytes` JSON. Runtime checks reject nonfinite floats,
unsafe integers, incorrect shapes, extra fields and input byte/array limits before
dispatch. SQL/body caps match the server. Inputs are copied/serialized immediately.
Origin URLs require HTTP(S), no credentials/query/hash or path prefix; project IDs
and keys require exact lowercase hex. Redirects fail, cookies/referrers are omitted.

Successful responses are validated too: typed rows and width, result/column/row
bounds, plans and status. Response reading is capped at 64 MiB and cancels rejected
streams. This is a wire cap, distinct from the engine's estimated 8 MiB row budget.
Unknown response fields currently require a compatible SDK update.

The older SQL methods use JavaScript numbers and support only safe integers and
transaction IDs, `[-(2^53-1), 2^53-1]` for signed values. They reject larger integer
responses even if a preceding write committed. Typed row methods instead preserve
full i64/u64 precision through decimal strings and finite float bits; see below.

Errors have `code`, optional `status`, and `outcome`:

| Outcome | Meaning |
| --- | --- |
| `not_started` | local validation/closed client/pre-dispatch cancellation |
| `not_committed` | fixed documented server refusal with its expected HTTP status |
| `unknown` | disconnect/timeout, uncertain server outcome or invalid success response |

No request retries occur. Inspect durable transaction state after `unknown`;
do not infer rollback from cancellation. Options accept an `AbortSignal` and
`timeoutMs` from 1 to 300000, with no default deadline. An in-progress blocking
commit may finish after abort. `close()` denies new calls; it cannot cancel a
request already dispatched. `setKey()` is an explicit key change, not refresh auth.
Private fields and safe serialization omit keys; this is not memory encryption.

The browser build uses standard Fetch APIs, but the current server has no CORS
configuration. Browser access requires a same-origin development reverse proxy.
Browser runtime/device checks, realtime/uploads/sessions and Kotlin remain pending.

## Executed verification

Fifteen unit tests cover request/response validation, all values, safe integers,
binding literals, declared/streamed response caps, aborts, refusal status matching,
key replacement and absence of raw peer errors. Regressions first reproduced error
text reflection, uncancelled oversized streams and rejection of valid 127-byte
qualified join labels, then passed after fixes.

Eight integration cases (including their parent) use an actual Rust TCP server:
typed SQL/CRUD/NULLs, rollback, scoped access, key rotation, unsafe-i64 unknown
outcome and restart with durable rows/keys/IDs. These are synthetic correctness
checks, not a production or browser-security audit.


## Exact typed rows

These methods call the existing Rust row API; they do not implement storage or
transactions in JavaScript. Use a trusted server-side project service key. The
current API does not grant browser/mobile user-token access to public tables.

```ts
const primary: RowKey = { type: "integer", value: "9223372036854775807" };
await client.rowInsert("items", [primary, { type: "text", value: "synthetic item" }]);
const row = await client.rowGet("items", primary); // row array or null
const page = await client.rowPage("items", 10);
if (page.next) await client.rowPage("items", 10, page.next);
await client.rowUpdate("items", primary, [primary, { type: "text", value: "changed" }]);
await client.rowBatch("items", [
  { op: "delete", key: primary },
  { op: "insert", row: [primary, { type: "text", value: "replacement" }] },
]);
await client.rowDelete("items", primary);
```

Import RowKey, RowValue, RowWrite, RowPage, RowChanged and BatchChanged as types.
Rows follow schema column order; tables are created with SQL or the schema API.
Integer values use canonical signed decimal strings, never JS numbers or raw
BigInt JSON. Use `bigint.toString()` when constructing one locally. Float values
use `float_bits` with16 lowercase hex digits; finite patterns only, preserving
negative zero. Boolean/null/text/bytes retain their tagged forms. This is distinct
from the older SQL Value wire. Mutation transaction IDs are exact decimal strings.

All six methods validate/copy inputs before dispatch, including identifiers,
numeric/text/byte/column/page/batch bounds and65,536-byte JSON cap. Schema-specific
column types and4000-byte physical record limits remain server checks. Pages cap at
128 requested rows and65,536 response bytes, validate consistent row width and
continuation shape, and read current state without a cross-request snapshot.
Complete oversized responses refuse; rejected streams are cancelled. SQL keeps its
separate64 MiB response cap. Options and cancellation outcomes match SQL methods.

Batch accepts1..256 ordered writes to one table and returns changed-operation count
plus transaction string after one Rust commit. Late errors roll back the entire
packet. Known table_rejected400 and workers_busy503 are not_committed; uncertain
writes/disconnects/malformed success remain unknown. There are no automatic retries.

Four new unit cases cover copied payloads, exact numeric extrema, strict variants,
response widths/counts/IDs, byte-cap stream cancellation and refusal outcomes.
Initial tests reproduced acceptance of the older float tag and inconsistent row
widths before both were fixed. The new real-server case exercises every method,
late rollback, pagination, signed64-bit extrema, float-bit preservation and a
received-ACK SIGKILL/reopen on both local Rust binaries. External-container mode
uses the same methods while leaving restart control to its deployment probe.
