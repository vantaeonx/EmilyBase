import test from "node:test";
import assert from "node:assert/strict";
import { EmilyBaseClient, EmilyBaseError } from "../dist/index.js";

const project = "1".repeat(32),
  apiKey = "a".repeat(64);
const config = { url: "http://localhost:7000", project, apiKey };
const report = {
  transaction: 2,
  committed: true,
  results: [
    {
      columns: ["id", "text"],
      rows: [
        [
          { type: "integer", value: 1 },
          { type: "text", value: "literal" },
        ],
      ],
      affected: 0,
    },
  ],
};
const status = { transaction: 1, tables: 0, rows: 0 };
const response = (value, code = 200) =>
  new Response(JSON.stringify(value), {
    status: code,
    headers: { "content-type": "application/json" },
  });
const client = (value = report) =>
  new EmilyBaseClient({ ...config, fetch: async () => response(value) });
const rejects = async (promise, code, outcome) =>
  assert.rejects(
    promise,
    (error) =>
      error instanceof EmilyBaseError &&
      error.code === code &&
      error.outcome === outcome,
  );

test("parameters stay separate and transport sends only the scoped key in a header", async () => {
  const calls = [];
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async (url, request) => {
      calls.push({ url, request });
      return response(report);
    },
  });
  const literal = "'); DROP TABLE t; --";
  const result = await sdk.sql("SELECT * FROM t WHERE text=$1", [
    { type: "text", value: literal },
  ]);
  assert.deepEqual(result, report);
  assert.equal(calls.length, 1);
  const { url, request } = calls[0];
  assert.equal(url, `http://localhost:7000/v1/projects/${project}/sql`);
  assert.equal(
    new Headers(request.headers).get("authorization"),
    `Bearer ${apiKey}`,
  );
  assert.deepEqual(JSON.parse(request.body), {
    sql: "SELECT * FROM t WHERE text=$1",
    parameters: [{ type: "text", value: literal }],
  });
  assert.equal(request.redirect, "error");
  assert.equal(request.credentials, "omit");
  assert.equal(request.cache, "no-store");
  assert.equal(request.referrerPolicy, "no-referrer");
  assert.equal(request.method, "POST");
  assert(!url.includes(apiKey));
  assert(!JSON.stringify(sdk).includes(apiKey));
  assert(!String(sdk).includes(apiKey));
});
test("invalid origin IDs credentials and unsafe input fail before fetching", async () => {
  let calls = 0;
  const fetch = async () => {
    calls++;
    return response(report);
  };
  for (const options of [
    { ...config, url: "file:///tmp/db" },
    { ...config, url: "https://user:secret@example.com/" },
    { ...config, url: "https://example.com/?key=hidden" },
    { ...config, url: "https://example.com/#hidden" },
    { ...config, url: "https://example.com/prefix" },
    { ...config, project: "../escape" },
    { ...config, project: "A".repeat(32) },
    { ...config, apiKey: "bad" },
  ]) {
    assert.throws(
      () => new EmilyBaseClient({ ...options, fetch }),
      (error) =>
        error instanceof EmilyBaseError && error.outcome === "not_started",
    );
  }
  const sdk = new EmilyBaseClient({ ...config, fetch });
  for (const value of [
    { type: "integer", value: Number.MAX_SAFE_INTEGER + 1 },
    { type: "integer", value: 1.5 },
    { type: "float", value: Infinity },
    { type: "float", value: NaN },
    { type: "text", value: "é".repeat(1537) },
    { type: "bytes", value: [256] },
    { type: "bytes", value: [-1] },
    { type: "null", value: 1 },
    { type: "text", value: "ok", extra: true },
  ]) {
    assert.throws(
      () => sdk.sql("SELECT $1", [value]),
      (error) =>
        error instanceof EmilyBaseError && error.code === "invalid_input",
    );
  }
  assert.throws(() => sdk.sql("x".repeat(16385)));
  assert.throws(() =>
    sdk.sql(
      "SELECT $1",
      Array.from({ length: 257 }, () => ({ type: "null" })),
    ),
  );
  assert.throws(() =>
    sdk.sql(
      "SELECT $1",
      Array.from({ length: 25 }, () => ({
        type: "text",
        value: "x".repeat(3072),
      })),
    ),
  );
  assert.equal(calls, 0);
});
test("every catalog value decodes and input byte arrays are copied before awaiting", async () => {
  const values = [
    { type: "null" },
    { type: "boolean", value: true },
    { type: "integer", value: -Number.MAX_SAFE_INTEGER },
    { type: "float", value: 1e30 },
    { type: "text", value: "Привет 🦀" },
    { type: "bytes", value: [0, 255] },
  ];
  const good = {
    transaction: 3,
    committed: true,
    results: [
      { columns: values.map((_, i) => `c${i}`), rows: [values], affected: 0 },
    ],
  };
  const calls = [];
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async (_, request) => {
      calls.push(request.body);
      return response(good);
    },
  });
  const input = { type: "bytes", value: [1, 2] };
  const result = sdk.sql("SELECT $1", [input]);
  input.value[0] = 9;
  assert.deepEqual((await result).results[0].rows[0], values);
  assert.deepEqual(JSON.parse(calls[0]).parameters, [
    { type: "bytes", value: [1, 2] },
  ]);
});
test("malformed and rounded integer success responses leave write outcome unknown", async () => {
  for (const bad of [
    { ...report, transaction: Number.MAX_SAFE_INTEGER + 1 },
    { ...report, committed: "true" },
    { ...report, extra: "secret" },
    { ...report, results: [{ columns: ["id"], rows: [[]], affected: 0 }] },
    {
      ...report,
      results: [
        {
          columns: ["id"],
          rows: [[{ type: "integer", value: 1e20 }]],
          affected: 0,
        },
      ],
    },
    {
      ...report,
      results: [
        {
          columns: ["id"],
          rows: [[{ type: "float", value: null }]],
          affected: 0,
        },
      ],
    },
    {
      ...report,
      results: [
        {
          columns: ["id"],
          rows: [[{ type: "text", value: "x".repeat(3073) }]],
          affected: 0,
        },
      ],
    },
  ]) {
    await rejects(
      client(bad).sql("INSERT INTO t VALUES(1)"),
      "protocol_error",
      "unknown",
    );
  }
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () =>
      new Response(
        '{"transaction":9007199254740993,"committed":true,"results":[]}',
      ),
  });
  await rejects(
    sdk.sql("INSERT INTO t VALUES(1)"),
    "protocol_error",
    "unknown",
  );
});
test("status and explain decode their actual wire shapes without write endpoints", async () => {
  const calls = [];
  const plan = {
    access: "primary_key",
    table: "t",
    joined_table: null,
    sorted: false,
    limit: 10000,
  };
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async (url, request) => {
      calls.push(request);
      return response(url.endsWith("status") ? status : plan);
    },
  });
  assert.deepEqual(await sdk.status(), status);
  assert.equal(calls[0].method, "GET");
  assert.equal(calls[0].body, undefined);
  assert.deepEqual(
    await sdk.explain("SELECT * FROM t WHERE id=$1", [
      { type: "integer", value: 1 },
    ]),
    plan,
  );
  assert.equal(calls[1].method, "POST");
  const rangePlan = { ...plan, access: "primary_range" };
  assert.deepEqual(
    await client(rangePlan).explain("SELECT * FROM t WHERE id>=1 AND id<3"),
    rangePlan,
  );
  await rejects(
    client({ ...status, rows: 10001 }).status(),
    "protocol_error",
    "unknown",
  );
  await rejects(
    client({ ...plan, access: "fake" }).explain("SELECT * FROM t"),
    "protocol_error",
    "unknown",
  );
});
test("qualified star labels preserve maximum-length qualifiers and column names", async () => {
  const label = `${"a".repeat(63)}.${"b".repeat(63)}`;
  const good = {
    transaction: 2,
    committed: true,
    results: [
      {
        columns: [label],
        rows: [[{ type: "integer", value: 1 }]],
        affected: 0,
      },
    ],
  };
  assert.equal(
    (await client(good).sql("SELECT * FROM t AS a JOIN t AS b ON a.id=b.id"))
      .results[0].columns[0],
    label,
  );
});
test("fixed refusal codes are classified without automatic retries", async () => {
  for (const [code, status] of [
    ["access_denied", 401],
    ["query_rejected", 400],
    ["rate_limit", 429],
    ["workers_busy", 503],
    ["body_timeout", 408],
    ["json_required", 415],
  ]) {
    let calls = 0;
    const sdk = new EmilyBaseClient({
      ...config,
      fetch: async () => {
        calls++;
        return response({ code }, status);
      },
    });
    await rejects(sdk.sql("INSERT INTO t VALUES (1)"), code, "not_committed");
    assert.equal(calls, 1);
  }
  for (const code of [
    "transaction_outcome_requires_inspection",
    "publication_outcome_requires_inspection",
    "storage_unavailable",
  ]) {
    await rejects(
      new EmilyBaseClient({
        ...config,
        fetch: async () => response({ code }, 503),
      }).sql("INSERT INTO t VALUES (1)"),
      code,
      "unknown",
    );
  }
});
test("untrusted peer errors never copy arbitrary text or treat mismatched status as refusal", async () => {
  const privateText = "private_token_text";
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () => response({ code: privateText }, 503),
  });
  await rejects(sdk.sql("SELECT * FROM t"), "http_error", "unknown");
  const wrong = new EmilyBaseClient({
    ...config,
    fetch: async () => response({ code: "access_denied" }, 500),
  });
  await rejects(wrong.sql("SELECT * FROM t"), "access_denied", "unknown");
  const transport = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      throw new Error(apiKey + " private SQL");
    },
  });
  await assert.rejects(
    transport.sql("INSERT INTO t VALUES(1)"),
    (error) =>
      error.code === "transport_error" &&
      error.outcome === "unknown" &&
      !String(error).includes(apiKey),
  );
});
test("response size cap covers declared and streamed data and cancels readers", async () => {
  let cancelled = false;
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode("{}"));
    },
    cancel() {
      cancelled = true;
    },
  });
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () =>
      new Response(stream, {
        headers: { "content-length": String(64 * 1024 * 1024 + 1) },
      }),
  });
  await rejects(sdk.sql("SELECT * FROM t"), "response_limit", "unknown");
  assert(cancelled);
  const chunk = new Uint8Array(32 * 1024 * 1024);
  let sent = 0;
  const large = new ReadableStream({
    pull(controller) {
      controller.enqueue(++sent < 3 ? chunk : new Uint8Array(1));
    },
    cancel() {
      cancelled = true;
    },
  });
  await rejects(
    new EmilyBaseClient({
      ...config,
      fetch: async () => new Response(large),
    }).sql("SELECT * FROM t"),
    "response_limit",
    "unknown",
  );
});
test("cancellation before dispatch differs from an interrupted in-flight write", async () => {
  let calls = 0;
  const stopped = new AbortController();
  stopped.abort();
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async () => {
      calls++;
      return response(report);
    },
  });
  await rejects(
    sdk.sql("INSERT INTO t VALUES(1)", [], { signal: stopped.signal }),
    "cancelled",
    "not_started",
  );
  assert.equal(calls, 0);
  const hanging = new EmilyBaseClient({
    ...config,
    fetch: (_, options) =>
      new Promise((_, reject) => {
        calls++;
        options.signal.addEventListener(
          "abort",
          () => reject(new Error("aborted")),
          { once: true },
        );
      }),
  });
  await rejects(
    hanging.sql("INSERT INTO t VALUES(1)", [], { timeoutMs: 10 }),
    "cancelled",
    "unknown",
  );
  assert.equal(calls, 1);
  for (const timeoutMs of [0, -1, 0.5, 300001, Infinity])
    await rejects(sdk.status({ timeoutMs }), "invalid_input", "not_started");
});
test("explicit key replacement and closure preserve private serialization", async () => {
  const headers = [];
  const sdk = new EmilyBaseClient({
    ...config,
    fetch: async (_, request) => {
      headers.push(new Headers(request.headers));
      return response(status);
    },
  });
  await sdk.status();
  sdk.setKey("b".repeat(64));
  await sdk.status();
  assert.equal(headers[0].get("authorization"), `Bearer ${apiKey}`);
  assert.equal(headers[1].get("authorization"), `Bearer ${"b".repeat(64)}`);
  assert(!JSON.stringify(sdk).includes(apiKey));
  sdk.close();
  await rejects(sdk.status(), "client_closed", "not_started");
  assert.throws(
    () => sdk.setKey(apiKey),
    (error) => error.code === "client_closed",
  );
  assert.equal(headers.length, 2);
});
