# EmilyBase TypeScript client

Experimental project-scoped ESM SDK. Apache-2.0. No runtime dependency. The package
is local/private until a separate release; it is not published on npm. Node 22 is
the tested environment. Build from the repository root:

```sh
npm ci --ignore-scripts --prefix sdks/typescript
npm test --prefix sdks/typescript
cargo build --locked -p emilybase-cli -p emilybase-server
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
[HTTP API](../../docs/server.md). The SDK exposes project SQL, explain, status, typed row operations and bounded migrations. Project keys currently grant full access to one project;
this privileged client bypasses user-row policies. Keep service keys on a trusted
backend. A separate user client is described below. Use synthetic data and keep credentials private.
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
Browser runtime/device checks, realtime/uploads and Kotlin remain pending.

## Executed verification

Sixteen unit tests cover request/response validation, all values, safe integers,
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
service client has full project authority. The separate user client below applies
the current Rust user admission/session/row policies.

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


UTF-16 input strings with lone surrogates are rejected locally before TextEncoder
can replace them. Valid supplementary code points, NUL and bounded UTF-8 strings
retain their exact content. The regression first failed before this check; final
16 unit tests and8 real-server cases on each Rust binary pass.


## Bounded migrations

Use the trusted project service key on a backend. This client does not grant
end-user migration authority. The original Rust engine owns parsing, schema/data
writes, WAL commits and recovery; TypeScript only validates and transports requests.

```ts
const definition = {
  version: 1,
  label: "initial",
  sql: "CREATE TABLE notes(id INT PRIMARY KEY,title TEXT)",
};
const applied = await client.migrationApply(definition);
const receipts = await client.migrationList();
// An explicit exact historical retry returns the original receipt without a write.
const repeated = await client.migrationApply(definition);
```

Definitions have exact fields, version1..128, ASCII label1..63 and SQL1..16,384 UTF-8
bytes. Unpaired UTF-16 surrogates refuse before encoding so exact definition bytes
cannot silently change. JSON bodies and streamed responses cap at65,536 bytes.
The client snapshots input before dispatch; the Rust parser remains authoritative
for SQL syntax, statement/work/event bounds and reserved-ledger access.

Receipt transaction IDs remain canonical decimal strings through the full u64
range; do not convert them to JavaScript numbers. Digests use64 lowercase hex
characters. Inventory checks consecutive versions, at most128 entries and strictly
increasing commit IDs. Apply checks receipt version/label against its request.
This is structural response validation; the SDK does not recompute the SQL digest
or authenticate the owner-writable ledger as an audit log.

A400 `migration_rejected` is an ordinary no-commit refusal. Invalid existing ledger
(`migration_history_invalid`), malformed/truncated response, lost connection and
ambiguous commit remain conservative unknown outcomes. Never infer rollback from
a timeout or lost reply. Reopen/inspect and explicitly retry the exact definition;
the client never retries automatically or changes versions/SQL to hide an error.
There is no migration file loader, secret persistence, dashboard, ALTER/schema diff,
down/large/online orchestration or npm release in this increment.


## Explicit user client

Use EmilyBaseUserClient for admitted user HTTP. The project must have existing
users, policies and an explicitly opened v5 admission flag. Never ship a project
service key to end users. This separate client accepts only origin/project/fetch;
it has no SQL, schema, migration or administrator methods.

```ts
import { EmilyBaseUserClient } from "./dist/index.js";
const users = new EmilyBaseUserClient({ url: "http://127.0.0.1:7000", project: projectId });
let pair = await users.signIn(loginFromForm, exactPasswordFromForm);
const me = await users.me(pair.access_token);
const page = await users.rowPage(pair.access_token, "owned", 10);
await users.rowWrite(pair.access_token, "owned", [
  { op: "delete", key: { type: "integer", value: "7" } },
]);
pair = await users.refresh(pair.refresh_token); // deliberate single-use refresh
await users.logout(pair.refresh_token);
users.close();
```

rowGet(access, table, key), rowPage(access, table, limit, after) and
rowWrite(access, table, operations) reuse exact RowKey/RowValue/RowWrite types.
All methods accept RequestOptions last. Import UserClientOptions, UserSession and
UserInfo as types. Session expiry and credential epoch remain exact decimal strings.
Rows follow server schema column order; use trusted metadata for an owner ID.

No credentials are stored by the client, no automatic retry/refresh occurs, and
returned plaintext pairs belong to the caller. Do not log/serialize them. The
client itself serializes only project/closed metadata; it cannot wipe application
copies or immutable JavaScript strings. Token parsing checks purpose/shape only;
the original Rust private owner decides current authority on every operation.

Passwords preserve exact Unicode UTF-8 without trimming, with1..1024-byte and4096
escaped JSON limits; invalid UTF-16 refuses locally. Session replies cap at4096,
row requests/replies at65536 bytes. Row writes bind receipt count to the copied
packet, preserving i64/u64 digits and float bits through existing row validators.

This client uses not_started for local rejection, and conservative unknown for
remote errors, disconnects, malformed replies or cancellation after dispatch.
The private clock may commit separately even when a row packet refuses. A lost
single-use refresh result can require signing in again. Never infer rollback or
retry automatically; inspect the application's durable state after a lost write.
The original privileged client retains its existing refusal classification.

The expanded integration script includes private-root user scenarios using actual
Rust CLI/server binaries. EMILYBASE_CLI_BIN and EMILYBASE_SERVER_BIN can select
explicit locally built binaries. An external EMILYBASE_TEST_URL remains the legacy
registry probe and explicitly skips private-root provisioning. No npm release or
browser/device/CORS/cookie/TLS contract is established here. See the full
[user contract](../../docs/user-sdk.md).


The admitted changePassword(access, currentPassword, replacementPassword, options)
method changes only the current token owner and returns UserInfo. Both password
strings preserve exact UTF-8 and the combined4096-byte escaped JSON cap. A successful
original epoch commit revokes every older family, including the calling access.
No pair is returned or stored: sign in explicitly using the intended replacement.
The client accepts no login/user-ID override and never automatically repeats a lost
change. See [password contract](../../docs/user-password-change.md).
