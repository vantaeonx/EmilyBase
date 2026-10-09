import test from "node:test";
import assert from "node:assert/strict";
import { EmilyBaseUserClient, EmilyBaseError } from "../dist/index.js";

const project = "1".repeat(32);
const token = (kind, family = "2", secret = "a") =>
  `eb${kind}1_${family.repeat(32)}.${secret.repeat(64)}`;
const access = token("a"),
  refresh = token("r");
const session = {
  access_token: access,
  refresh_token: refresh,
  token_type: "Bearer",
  expires_at: "9223372036854775807",
};
const user = {
  id: "3".repeat(32),
  login: "synthetic_user",
  credential_epoch: "18446744073709551615",
  disabled: false,
};
const config = { url: "http://localhost:7000", project };
const integer = (value) => ({ type: "integer", value: String(value) });
const response = (value, status = 200, headers = {}) =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json", ...headers },
  });
const invalid = (error) =>
  error instanceof EmilyBaseError &&
  error.code === "invalid_input" &&
  error.outcome === "not_started";
const rejects = (promise, code, outcome = "unknown") =>
  assert.rejects(
    promise,
    (error) =>
      error instanceof EmilyBaseError &&
      error.code === code &&
      error.outcome === outcome,
  );

test("user wire sends explicit purpose tokens with no service or cookie authority", async () => {
  const calls = [];
  let value = session;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async (url, request) => {
      calls.push({ url, request });
      return response(value);
    },
  });
  const password = " \0界\u{1f600}exact\n ";
  assert.deepEqual(await client.signIn("synthetic_user", password), session);
  assert.deepEqual(await client.refresh(refresh), session);
  value = user;
  assert.deepEqual(await client.me(access), user);
  value = { row: [integer(-9223372036854775808n)] };
  assert.deepEqual(
    await client.rowGet(access, "owned", integer(-9223372036854775808n)),
    value.row,
  );
  value = {
    rows: [[integer(9223372036854775807n)]],
    next: integer(9223372036854775807n),
  };
  assert.deepEqual(await client.rowPage(access, "owned", 1), value);
  value = { changed: 1, transaction: "18446744073709551615" };
  assert.deepEqual(
    await client.rowWrite(access, "owned", [{ op: "delete", key: integer(1) }]),
    value,
  );
  value = { logged_out: true };
  assert.deepEqual(await client.logout(refresh), value);
  assert.deepEqual(
    calls.map((c) => c.url),
    [
      "sign-in",
      "refresh",
      "me",
      "rows/get",
      "rows/page",
      "rows/write",
      "logout",
    ].map((route) => `${config.url}/v1/projects/${project}/user/${route}`),
  );
  assert.deepEqual(JSON.parse(calls[0].request.body), {
    login: "synthetic_user",
    password,
  });
  assert.deepEqual(JSON.parse(calls[1].request.body), {
    refresh_token: refresh,
  });
  assert.equal(calls[2].request.body, "{}");
  for (const [i, { request }] of calls.entries()) {
    const headers = new Headers(request.headers);
    assert.equal(
      headers.get("authorization"),
      [0, 1, 6].includes(i) ? null : `Bearer ${access}`,
    );
    assert.equal(headers.has("x-emilybase-access"), false);
    assert.equal(request.method, "POST");
    assert.equal(request.redirect, "error");
    assert.equal(request.credentials, "omit");
    assert.equal(request.cache, "no-store");
    assert.equal(request.referrerPolicy, "no-referrer");
  }
  assert.deepEqual(JSON.parse(JSON.stringify(client)), {
    project,
    closed: false,
  });
  for (const secret of [password, access, refresh])
    assert(
      !String(client).includes(secret) &&
        !JSON.stringify(client).includes(secret),
    );
});

test("user constructor rejects service secrets and unsafe origins locally", () => {
  for (const options of [
    null,
    [],
    { ...config, apiKey: "a".repeat(64) },
    { ...config, project: "../private" },
    { ...config, project: project.toUpperCase().replace("1", "G") },
    { ...config, fetch: 1 },
    ...[
      "file:///private",
      "https://user:password@example.test/",
      "https://example.test/prefix",
      "https://example.test/?token=synthetic",
      "https://example.test/#synthetic",
    ].map((url) => ({ ...config, url })),
  ])
    assert.throws(() => new EmilyBaseUserClient(options), invalid);
});

test("user input rejects wrong purposes missing tokens and noncanonical values before fetch", () => {
  let calls = 0;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async () => {
      calls++;
      return response(session);
    },
  });
  for (const value of [
    undefined,
    null,
    "a".repeat(64),
    refresh,
    access.toUpperCase(),
    access + "\n",
    "界".repeat(102),
  ]) {
    assert.throws(() => client.me(value), invalid);
    assert.throws(() => client.rowGet(value, "owned", integer(1)), invalid);
  }
  for (const value of [
    undefined,
    access,
    "a".repeat(64),
    refresh.slice(1),
    refresh.replace(".", "-"),
  ]) {
    assert.throws(() => client.refresh(value), invalid);
    assert.throws(() => client.logout(value), invalid);
  }
  for (const login of [
    "",
    "Upper",
    " padded",
    "synthetic\n",
    "a".repeat(65),
    "界",
  ])
    assert.throws(() => client.signIn(login, "synthetic"), invalid);
  for (const password of [
    "",
    "a".repeat(1025),
    "界".repeat(342),
    "\ud800",
    "\udc00",
    "ok\ud800tail",
    "\0".repeat(1024),
  ])
    assert.throws(() => client.signIn("synthetic", password), invalid);
  for (const work of [
    () => client.rowPage(access, "t", 0),
    () => client.rowPage(access, "t", 129),
    () => client.rowGet(access, "../private", integer(1)),
    () => client.rowGet(access, "t", integer("-0")),
    () => client.rowWrite(access, "t", []),
    () =>
      client.rowWrite(
        access,
        "t",
        Array(257).fill({ op: "delete", key: integer(1) }),
      ),
    () =>
      client.rowWrite(access, "t", [
        { op: "insert", row: [{ type: "text", value: "\ud800" }] },
      ]),
  ])
    assert.throws(work, invalid);
  assert.equal(calls, 0);
});

test("user row packets and receipt counts bind an immediate input snapshot", async () => {
  let request;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async (_url, r) => {
      request = r;
      await Promise.resolve();
      return response({ changed: 1, transaction: "1" });
    },
  });
  const bytes = [0, 255];
  const operations = [
    { op: "insert", row: [integer(1), { type: "bytes", value: bytes }] },
  ];
  const promised = client.rowWrite(access, "owned", operations);
  bytes.push(7);
  operations.push({ op: "delete", key: integer(1) });
  assert.deepEqual(await promised, { changed: 1, transaction: "1" });
  assert.deepEqual(JSON.parse(request.body).operations, [
    { op: "insert", row: [integer(1), { type: "bytes", value: [0, 255] }] },
  ]);
});

test("user session responses enforce exact shapes purposes same family and integer clock", async () => {
  let result;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async () => response(result),
  });
  for (const bad of [
    [],
    [access, refresh, "Bearer", "1"],
    { ...session, access_token: refresh },
    { ...session, refresh_token: access },
    { ...session, refresh_token: token("r", "3") },
    { ...session, token_type: "bearer" },
    { ...session, expires_at: 1 },
    { ...session, expires_at: "0" },
    { ...session, expires_at: "01" },
    { ...session, expires_at: "9223372036854775808" },
    { ...session, extra: "synthetic-sensitive" },
  ]) {
    result = bad;
    await rejects(client.signIn("synthetic", "password"), "protocol_error");
  }
  for (const bad of [
    null,
    [],
    { ...user, id: "x".repeat(32) },
    { ...user, credential_epoch: "0" },
    { ...user, credential_epoch: "18446744073709551616" },
    { ...user, disabled: 0 },
    { ...user, login: "Upper" },
    { ...user, extra: true },
  ]) {
    result = bad;
    await rejects(client.me(access), "protocol_error");
  }
  for (const bad of [
    { logged_out: false },
    { logged_out: true, extra: true },
    [true],
  ]) {
    result = bad;
    await rejects(client.logout(refresh), "protocol_error");
  }
});

test("user row replies enforce page limits exact bits and request packet count", async () => {
  let result;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async () => response(result),
  });
  for (const bad of [
    { rows: [], next: integer(1) },
    { rows: [[integer(1)], [integer(2)]], next: null },
    { rows: [[{ type: "integer", value: 1 }]], next: null },
    { rows: [[{ type: "float_bits", value: "7ff0000000000000" }]], next: null },
  ]) {
    result = bad;
    await rejects(client.rowPage(access, "owned", 1), "protocol_error");
  }
  for (const bad of [
    { changed: 2, transaction: "1" },
    { changed: 1, transaction: "0" },
    { changed: 1, transaction: 1 },
  ]) {
    result = bad;
    await rejects(
      client.rowWrite(access, "owned", [{ op: "delete", key: integer(1) }]),
      "protocol_error",
    );
  }
  result = { row: null };
  assert.equal(await client.rowGet(access, "owned", integer(1)), null);
});

test("user peer refusals and lost responses are conservative and never retried", async () => {
  let calls = 0,
    result,
    status;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async () => {
      calls++;
      return response(result, status);
    },
  });
  for (const [code, s] of [
    ["access_denied", 401],
    ["user_row_rejected", 403],
    ["invalid_account_request", 400],
    ["workers_busy", 503],
    ["session_outcome_requires_inspection", 503],
    ["user_row_outcome_requires_inspection", 503],
    ["rate_limit", 429],
  ]) {
    result = { code };
    status = s;
    await rejects(client.refresh(refresh), code);
  }
  assert.equal(calls, 7);
  for (const body of [
    { code: "synthetic-sensitive-token" },
    { code: "access_denied", detail: "synthetic-sensitive-token" },
    ["access_denied"],
  ]) {
    result = body;
    status = 401;
    await assert.rejects(
      client.logout(refresh),
      (error) =>
        error.code === "http_error" &&
        error.outcome === "unknown" &&
        !String(error).includes("synthetic-sensitive-token"),
    );
  }
  let lost = 0;
  const uncertain = new EmilyBaseUserClient({
    ...config,
    fetch: async () => {
      lost++;
      throw new Error("synthetic-sensitive-transport");
    },
  });
  await rejects(
    uncertain.rowWrite(access, "owned", [{ op: "delete", key: integer(1) }]),
    "transport_error",
  );
  await rejects(uncertain.refresh(refresh), "transport_error");
  assert.equal(lost, 2);
});

test("user response caps cancel both declared and streamed oversized replies", async () => {
  for (const [maximum, call] of [
    [4096, (c) => c.refresh(refresh)],
    [65536, (c) => c.rowGet(access, "owned", integer(1))],
  ]) {
    for (const declared of [false, true]) {
      let cancelled = false;
      const client = new EmilyBaseUserClient({
        ...config,
        fetch: async () =>
          new Response(
            new ReadableStream({
              pull(c) {
                c.enqueue(new Uint8Array(maximum + 1));
              },
              cancel() {
                cancelled = true;
              },
            }),
            {
              headers: declared
                ? { "content-length": String(maximum + 1) }
                : {},
            },
          ),
      });
      await rejects(call(client), "response_limit");
      assert.equal(cancelled, true);
    }
  }
});

test("user cancellation and close distinguish local and dispatched work", async () => {
  let calls = 0;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async (_url, request) => {
      calls++;
      return new Promise((_, reject) =>
        request.signal.addEventListener(
          "abort",
          () => reject(new Error("synthetic-private-abort")),
          { once: true },
        ),
      );
    },
  });
  const controller = new AbortController();
  controller.abort();
  for (const options of [null, [], 1, { signal: "synthetic-invalid-signal" }])
    await rejects(client.me(access, options), "invalid_input", "not_started");
  await rejects(
    client.me(access, { signal: controller.signal }),
    "cancelled",
    "not_started",
  );
  for (const timeoutMs of [0, 300001, NaN, 1.5])
    await rejects(
      client.me(access, { timeoutMs }),
      "invalid_input",
      "not_started",
    );
  assert.equal(calls, 0);
  await rejects(client.me(access, { timeoutMs: 5 }), "cancelled");
  assert.equal(calls, 1);
  client.close();
  assert.throws(
    () => client.me(access),
    (e) => e.code === "client_closed" && e.outcome === "not_started",
  );
  assert.deepEqual(client.toJSON(), { project, closed: true });
});

test("user credentials preserve generated Unicode byte boundaries without normalization", async () => {
  let actual;
  const client = new EmilyBaseUserClient({
    ...config,
    fetch: async (_url, request) => {
      actual = JSON.parse(request.body);
      return response(session);
    },
  });
  for (let i = 1; i <= 128; i++) {
    const password = (i % 2 ? "界" : "\u{1f600}").repeat(i) + "\0e\u0301 ";
    await client.signIn("synthetic_user", password);
    assert.equal(actual.password, password);
    assert(Buffer.byteLength(password) <= 1024);
  }
  await client.signIn("synthetic_user", "\u{1f600}".repeat(256));
  assert.equal(Buffer.byteLength(actual.password), 1024);
  assert.throws(
    () => client.signIn("synthetic_user", "\u{1f600}".repeat(256) + "a"),
    invalid,
  );
});
