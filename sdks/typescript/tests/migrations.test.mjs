import test from "node:test";
import assert from "node:assert/strict";
import { EmilyBaseClient, EmilyBaseError } from "../dist/index.js";
const config = {
  url: "http://localhost:7000",
  project: "1".repeat(32),
  apiKey: "a".repeat(64),
};
const definition = {
  version: 1,
  label: "initial",
  sql: "CREATE TABLE t(id INT PRIMARY KEY)",
};
const receipt = (version = 1, transaction = "2") => ({
  version,
  label: "initial",
  sha256: "a".repeat(64),
  transaction,
});
const response = (value, status = 200) =>
  new Response(JSON.stringify(value), { status });
const rejected = (work, code, outcome) =>
  assert.rejects(
    work,
    (e) =>
      e instanceof EmilyBaseError && e.code === code && e.outcome === outcome,
  );
test("migration SDK scopes both routes, snapshots input and preserves full u64 receipts", async () => {
  const calls = [];
  let value = {
    receipt: receipt(1, "18446744073709551615"),
    already_applied: false,
  };
  const client = new EmilyBaseClient({
    ...config,
    fetch: async (url, request) => {
      calls.push({ url, request });
      return response(value);
    },
  });
  const input = { ...definition };
  const pending = client.migrationApply(input);
  input.sql = "DROP TABLE t";
  input.label = "changed";
  assert.deepEqual(await pending, value);
  assert.deepEqual(JSON.parse(calls[0].request.body), definition);
  assert.equal(
    calls[0].url,
    `${config.url}/v1/projects/${config.project}/migrations/apply`,
  );
  const headers = new Headers(calls[0].request.headers);
  assert.equal(headers.get("authorization"), `Bearer ${config.apiKey}`);
  assert.equal(headers.get("content-type"), "application/json");
  assert.equal(calls[0].request.redirect, "error");
  assert.equal(calls[0].request.cache, "no-store");
  assert.equal(calls[0].request.credentials, "omit");
  value = { migrations: [receipt()] };
  assert.deepEqual(await client.migrationList(), value.migrations);
  assert.equal(
    calls[1].url,
    `${config.url}/v1/projects/${config.project}/migrations`,
  );
  assert.equal(calls[1].request.method, "GET");
  assert.equal(calls[1].request.body, undefined);
  assert(!JSON.stringify(client).includes(config.apiKey));
  assert(!String(client).includes(definition.sql));
});
test("migration SDK rejects strict identity, UTF8 and escaped body bounds before fetch", () => {
  let calls = 0;
  const client = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      calls++;
      return response({});
    },
  });
  const cases = [
    null,
    {},
    [],
    { ...definition, path: "elsewhere" },
    { ...definition, parameters: [] },
    ...[-1, 0, 129, 1.5, NaN, Infinity, "1"].map((version) => ({
      ...definition,
      version,
    })),
    ...["", "../escape", " space", "a".repeat(64), "界", "x\n"].map(
      (label) => ({ ...definition, label }),
    ),
    ...[
      "",
      "x".repeat(16385),
      "界".repeat(5462),
      "\ud800",
      "\udfff",
      "x\ud800y",
      "\0".repeat(16384),
    ].map((sql) => ({ ...definition, sql })),
  ];
  for (const input of cases)
    assert.throws(
      () => client.migrationApply(input),
      (e) =>
        e instanceof EmilyBaseError &&
        e.code === "invalid_input" &&
        e.outcome === "not_started",
    );
  assert.equal(calls, 0);
});
test("migration SDK preserves valid supplementary unicode and exact byte boundary", async () => {
  const bodies = [];
  const client = new EmilyBaseClient({
    ...config,
    fetch: async (url, request) => {
      bodies.push(JSON.parse(request.body));
      return response({ receipt: receipt(), already_applied: false });
    },
  });
  for (const sql of [
    "x".repeat(16384),
    "界".repeat(5461) + "x",
    "😀".repeat(4096),
    "synthetic\0'😀",
  ]) {
    await client.migrationApply({ ...definition, sql });
    assert.equal(bodies.at(-1).sql, sql);
  }
});
test("migration receipt inventories obey an independent ordered metadata model", async () => {
  let value = { migrations: [] };
  const client = new EmilyBaseClient({
    ...config,
    fetch: async () => response(value),
  });
  for (let count = 0; count <= 128; count++) {
    const model = Array.from({ length: count }, (_, i) => ({
      ...receipt(i + 1, String(2 + 3 * i)),
      label: "a".repeat(63),
    }));
    value = { migrations: model };
    assert.deepEqual(await client.migrationList(), model);
  }
});
test("migration SDK rejects malformed, gapped, unordered and imprecise receipts", async () => {
  let value;
  const client = new EmilyBaseClient({
    ...config,
    fetch: async () => response(value),
  });
  const invalid = [
    null,
    {},
    { migrations: [], extra: true },
    { migrations: Array(129).fill(receipt()) },
    { migrations: [receipt(2)] },
    { migrations: [receipt(), receipt(2, "2")] },
    { migrations: [receipt(), receipt(2, "1")] },
  ];
  for (const transaction of [
    2,
    1,
    "1",
    "02",
    "+2",
    "2.0",
    "18446744073709551616",
    "-2",
    " 2",
  ])
    invalid.push({ migrations: [{ ...receipt(), transaction }] });
  for (const sha256 of ["", "A".repeat(64), "a".repeat(63), "g".repeat(64)])
    invalid.push({ migrations: [{ ...receipt(), sha256 }] });
  invalid.push(
    { migrations: [{ ...receipt(), sql: "sensitive" }] },
    { migrations: [{ ...receipt(), label: "../path" }] },
  );
  for (const input of invalid) {
    value = input;
    await rejected(client.migrationList(), "protocol_error", "unknown");
  }
  for (const input of [
    { receipt: receipt(2), already_applied: false },
    { receipt: { ...receipt(), label: "different" }, already_applied: false },
    { receipt: receipt(), already_applied: 1 },
    { receipt: receipt(), already_applied: false, sql: "sensitive" },
  ]) {
    value = input;
    await rejected(
      client.migrationApply(definition),
      "protocol_error",
      "unknown",
    );
  }
});
test("migration errors preserve safe refusals and ambiguous outcomes without retries or peer text", async () => {
  let value,
    status,
    calls = 0;
  const client = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      calls++;
      return response(value, status);
    },
  });
  for (const [code, http, outcome] of [
    ["migration_rejected", 400, "not_committed"],
    ["migration_rejected", 503, "unknown"],
    ["migration_history_invalid", 503, "unknown"],
    ["transaction_outcome_requires_inspection", 503, "unknown"],
    ["workers_busy", 503, "not_committed"],
  ]) {
    value = { code };
    status = http;
    const before = calls;
    await rejected(client.migrationApply(definition), code, outcome);
    assert.equal(calls, before + 1);
  }
  value = { code: "synthetic-peer-secret" };
  status = 503;
  await assert.rejects(
    client.migrationApply(definition),
    (e) =>
      e.code === "http_error" &&
      e.outcome === "unknown" &&
      !String(e).includes("synthetic-peer-secret") &&
      !String(e).includes(definition.sql),
  );
});
test("migration SDK bounds streamed response and handles cancellation and lost response conservatively", async () => {
  let calls = 0;
  const cancelled = new AbortController();
  cancelled.abort();
  const client = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      calls++;
      throw new Error("synthetic-sensitive-transport");
    },
  });
  await rejected(
    client.migrationApply(definition, { signal: cancelled.signal }),
    "cancelled",
    "not_started",
  );
  assert.equal(calls, 0);
  await rejected(
    client.migrationApply(definition),
    "transport_error",
    "unknown",
  );
  assert.equal(calls, 1);
  const huge = new EmilyBaseClient({
    ...config,
    fetch: async () => new Response(" ".repeat(65537)),
  });
  await rejected(huge.migrationList(), "response_limit", "unknown");
  await rejected(huge.migrationApply(definition), "response_limit", "unknown");
  const badUtf8 = new EmilyBaseClient({
    ...config,
    fetch: async () => new Response(new Uint8Array([0xff])),
  });
  await rejected(badUtf8.migrationList(), "transport_error", "unknown");
  client.close();
  await rejected(client.migrationList(), "client_closed", "not_started");
});
