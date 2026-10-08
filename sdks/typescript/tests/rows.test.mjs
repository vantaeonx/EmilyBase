import test from "node:test";
import assert from "node:assert/strict";
import { EmilyBaseClient, EmilyBaseError } from "../dist/index.js";
const project = "1".repeat(32),
  apiKey = "a".repeat(64);
const integer = (value) => ({ type: "integer", value: String(value) });
const config = { url: "http://localhost:7000", project, apiKey };
const response = (value, status = 200) =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
const rejects = (work, code, outcome) =>
  assert.rejects(
    work,
    (error) =>
      error instanceof EmilyBaseError &&
      error.code === code &&
      error.outcome === outcome,
  );
test("exact row routes serialize snapshots and keep scoped transport credentials", async () => {
  const calls = [];
  let result = {
    key: integer(9223372036854775807n),
    transaction: "18446744073709551615",
  };
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async (url, request) => {
      calls.push({ url, request });
      return response(result);
    },
  });
  const row = [
    integer(9223372036854775807n),
    { type: "float_bits", value: "8000000000000000" },
    { type: "bytes", value: [0, 255] },
    { type: "text", value: "synthetic'界\0" },
  ];
  const promised = sdk.rowInsert("items", row);
  row[2].value.push(7);
  assert.deepEqual(await promised, result);
  assert.equal(calls.length, 1);
  assert.equal(
    calls[0].url,
    `${config.url}/v1/projects/${project}/tables/rows/insert`,
  );
  assert.deepEqual(JSON.parse(calls[0].request.body).row[2].value, [0, 255]);
  assert.equal(
    new Headers(calls[0].request.headers).get("authorization"),
    `Bearer ${apiKey}`,
  );
  assert.equal(calls[0].request.cache, "no-store");
  assert.equal(calls[0].request.redirect, "error");
  assert.equal(calls[0].request.credentials, "omit");
  await sdk.rowUpdate("items", row[0], row);
  await sdk.rowDelete("items", row[0]);
  result = { row };
  assert.deepEqual(await sdk.rowGet("items", row[0]), row);
  result = { row: null };
  assert.equal(await sdk.rowGet("items", integer(1)), null);
  result = { rows: [row], next: row[0] };
  assert.deepEqual(await sdk.rowPage("items", 1, null), result);
  result = { changed: 2, transaction: "10" };
  assert.deepEqual(
    await sdk.rowBatch("items", [
      { op: "insert", row },
      { op: "delete", key: row[0] },
    ]),
    result,
  );
  assert(!JSON.stringify(sdk).includes(apiKey));
  assert.deepEqual(
    calls.map((c) => c.url.split("/").at(-1)),
    ["insert", "update", "delete", "get", "get", "page", "batch"],
  );
});
test("row wire rejects unsafe alternate floats and invalid bounds before fetch", () => {
  let calls = 0;
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      calls++;
      return response({ key: integer(1), transaction: "1" });
    },
  });
  const bad = [
    { type: "float", value: 1 },
    { type: "integer", value: 1 },
    integer("+1"),
    integer("-0"),
    integer("01"),
    integer("9223372036854775808"),
    { type: "float_bits", value: "7ff0000000000000" },
    { type: "float_bits", value: "fff8000000000001" },
    { type: "float_bits", value: "3FF0000000000000" },
    { type: "text", value: "界".repeat(1025) },
    { type: "text", value: "\ud800" },
    { type: "text", value: "\udfff" },
    { type: "bytes", value: [256] },
    { type: "null", extra: "synthetic-private" },
  ];
  for (const value of bad)
    assert.throws(
      () => sdk.rowInsert("items", [integer(1), value]),
      (error) =>
        error instanceof EmilyBaseError &&
        error.code === "invalid_input" &&
        error.outcome === "not_started",
    );
  for (const work of [
    () => sdk.rowGet("../private", integer(1)),
    () => sdk.rowPage("t", 0),
    () => sdk.rowPage("t", 129),
    () => sdk.rowInsert("t", []),
    () => sdk.rowInsert("t", Array(65).fill({ type: "null" })),
    () => sdk.rowBatch("t", []),
    () => sdk.rowBatch("t", Array(257).fill({ op: "delete", key: integer(1) })),
    () =>
      sdk.rowBatch("t", [
        { op: "insert", row: [integer(1)], project: "other" },
      ]),
    () =>
      sdk.rowInsert(
        "t",
        Array(30).fill({ type: "text", value: "\0".repeat(3072) }),
      ),
  ])
    assert.throws(
      work,
      (error) =>
        error instanceof EmilyBaseError && error.outcome === "not_started",
    );
  assert.equal(calls, 0);
});
test("row responses reject inconsistent widths bad identifiers and invalid exact values", async () => {
  let value;
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () => response(value),
  });
  for (const invalid of [
    { rows: [[integer(1)], [integer(2), { type: "null" }]], next: null },
    { rows: [], next: integer(1) },
    { rows: [[{ type: "float", value: 1 }]], next: null },
    { rows: [[integer("9223372036854775808")]], next: null },
    { rows: [], next: null, private: "synthetic-private" },
  ]) {
    value = invalid;
    await rejects(sdk.rowPage("t", 1), "protocol_error", "unknown");
  }
  for (const invalid of [
    { key: integer(1), transaction: "0" },
    { key: integer(1), transaction: "18446744073709551616" },
    { key: integer(1), transaction: 1 },
    { key: integer(1), transaction: "1", extra: true },
  ]) {
    value = invalid;
    await rejects(sdk.rowDelete("t", integer(1)), "protocol_error", "unknown");
  }
  value = { changed: 257, transaction: "1" };
  await rejects(
    sdk.rowBatch("t", [{ op: "delete", key: integer(1) }]),
    "protocol_error",
    "unknown",
  );
});
test("row response byte caps cancel streams and fixed refusals preserve outcome", async () => {
  let cancelled = false;
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () =>
      new Response(
        new ReadableStream({
          pull(c) {
            c.enqueue(new Uint8Array(65537));
          },
          cancel() {
            cancelled = true;
          },
        }),
      ),
  });
  await rejects(sdk.rowGet("t", integer(1)), "response_limit", "unknown");
  assert.equal(cancelled, true);
  for (const status of [400, 503]) {
    const sdk = new EmilyBaseClient({
      ...config,
      fetch: async () =>
        response(
          { code: status === 400 ? "table_rejected" : "workers_busy" },
          status,
        ),
    });
    await rejects(
      sdk.rowBatch("t", [{ op: "delete", key: integer(1) }]),
      status === 400 ? "table_rejected" : "workers_busy",
      "not_committed",
    );
  }
  const uncertain = new EmilyBaseClient({
    ...config,
    fetch: async () =>
      response({ code: "transaction_outcome_requires_inspection" }, 503),
  });
  await rejects(
    uncertain.rowDelete("t", integer(1)),
    "transaction_outcome_requires_inspection",
    "unknown",
  );
});

test("valid supplementary Unicode and NUL survive exact row transport", async () => {
  const original = [
    { type: "integer", value: "1" },
    { type: "text", value: "\u{1f600}\u{10ffff}\0界" },
  ];
  let input;
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async (_url, request) => {
      input = JSON.parse(request.body);
      return response({ row: original });
    },
  });
  assert.deepEqual(
    await sdk.rowGet("t", { type: "text", value: "\u{1f600}\0" }),
    original,
  );
  assert.equal(input.key.value, "\u{1f600}\0");
  let calls = 0;
  const denied = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      calls++;
      return response({ row: null });
    },
  });
  for (const value of ["\ud800", "\udc00", "ok\ud800tail", "\udc00\ud800"]) {
    assert.throws(
      () => denied.rowGet("t", { type: "text", value }),
      (error) =>
        error instanceof EmilyBaseError && error.outcome === "not_started",
    );
  }
  assert.equal(calls, 0);
});
