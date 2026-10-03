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
[HTTP API](../../docs/server.md). The SDK intentionally exposes only project SQL,
explain and status. Project keys currently grant full access to one project;
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

JavaScript numbers do not preserve all i64/u64 values. This SDK supports only safe
integer values and transaction IDs, `[-(2^53-1), 2^53-1]` for signed values. It
rejects larger integer responses even if a preceding write committed. Floats are
finite IEEE-754 values. Full integer-range transport/BigInt is a future API version.

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

Eleven unit tests cover request/response validation, all values, safe integers,
binding literals, declared/streamed response caps, aborts, refusal status matching,
key replacement and absence of raw peer errors. Regressions first reproduced error
text reflection, uncancelled oversized streams and rejection of valid 127-byte
qualified join labels, then passed after fixes.

Seven integration cases (including their parent) use an actual Rust TCP server:
typed SQL/CRUD/NULLs, rollback, scoped access, key rotation, unsafe-i64 unknown
outcome and restart with durable rows/keys/IDs. These are synthetic correctness
checks, not a production or browser-security audit.
