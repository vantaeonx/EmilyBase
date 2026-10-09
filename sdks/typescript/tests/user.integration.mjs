import test from "node:test";
import assert from "node:assert/strict";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { once } from "node:events";
import { mkdtemp, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { randomBytes } from "node:crypto";
import { EmilyBaseUserClient, EmilyBaseError } from "../dist/index.js";

const binary = (name) =>
  fileURLToPath(new URL(`../../../target/debug/${name}`, import.meta.url));
const execute = promisify(execFile);
const integer = (n) => ({ type: "integer", value: String(n) });
const denied = (promise) =>
  assert.rejects(
    promise,
    (error) =>
      error instanceof EmilyBaseError &&
      error.code === "access_denied" &&
      error.outcome === "unknown",
  );
const OWN = {
  version: 1,
  ...Object.fromEntries(
    ["select", "insert", "update_using", "update_check", "delete"].map(
      (name) => [name, { kind: "owner", column: "owner" }],
    ),
  ),
};

test(
  "user SDK executes against private Rust owners with both WALs",
  {
    timeout: 90000,
    skip: process.env.EMILYBASE_TEST_URL
      ? "external legacy-registry probe has no private-root provisioning authority"
      : false,
  },
  async (t) => {
    for (const compact of [false, true])
      await t.test(compact ? "compact WAL" : "original WAL", async (t) => {
        const directory = await mkdtemp(join(tmpdir(), "emilybase-user-sdk-"));
        const root = join(directory, "root"),
          keyFile = join(directory, "service.key");
        const master = randomBytes(32).toString("hex");
        const cli = process.env.EMILYBASE_CLI_BIN ?? binary("emilybase");
        const server =
          process.env.EMILYBASE_SERVER_BIN ?? binary("emilybase-server");
        const logs = [];
        const secrets = [
          master,
          "synthetic_user",
          "synthetic_other",
          "synthetic-private-user-password",
        ];
        let child, url, client;
        async function command(args, input) {
          if (input === undefined) {
            const out = await execute(cli, args, {
              timeout: 10000,
              maxBuffer: 65536,
            });
            assert.equal(out.stderr, "");
            return out.stdout;
          }
          const p = spawn(cli, args, { stdio: ["pipe", "pipe", "pipe"] });
          const output = [],
            errors = [];
          p.stdout.on("data", (chunk) => output.push(chunk));
          p.stderr.on("data", (chunk) => errors.push(chunk));
          const exited = once(p, "exit");
          p.stdin.end(input);
          const timer = setTimeout(() => p.kill("SIGKILL"), 10000);
          try {
            const [code] = await exited;
            assert.equal(code, 0, "offline command succeeds");
            assert.equal(Buffer.concat(errors).length, 0);
            return Buffer.concat(output).toString();
          } finally {
            clearTimeout(timer);
          }
        }
        async function start(path = root) {
          child = spawn(server, [], {
            env: {
              ...process.env,
              EMILYBASE_MASTER_KEY: master,
              EMILYBASE_MASTER_KEY_FILE: undefined,
              EMILYBASE_DATA_DIR: undefined,
              EMILYBASE_ACCOUNT_ROOT: path,
              EMILYBASE_LISTEN: "127.0.0.1:0",
            },
            stdio: ["ignore", "pipe", "pipe"],
          });
          child.stderr.on("data", (chunk) => logs.push(chunk.toString()));
          let timer;
          try {
            url = await Promise.race([
              new Promise((resolve, reject) => {
                child.once("error", () =>
                  reject(new Error("user server startup failed")),
                );
                child.once("exit", () =>
                  reject(new Error("user server exited during startup")),
                );
                createInterface({ input: child.stdout }).on("line", (line) => {
                  logs.push(line);
                  const r = JSON.parse(line);
                  if (r.fields?.message === "experimental_server_listening")
                    resolve(`http://${r.fields.address}`);
                });
              }),
              new Promise((_, reject) => {
                timer = setTimeout(
                  () => reject(new Error("user server startup deadline")),
                  10000,
                );
              }),
            ]);
          } finally {
            clearTimeout(timer);
          }
          client = new EmilyBaseUserClient({ url, project });
        }
        async function stop(kill = false) {
          if (!child || child.exitCode !== null || child.signalCode !== null)
            return;
          const exited = once(child, "exit");
          child.kill(kill ? "SIGKILL" : "SIGTERM");
          const timer = setTimeout(() => child.kill("SIGKILL"), 10000);
          try {
            const [code, signal] = await exited;
            if (kill) assert.equal(signal, "SIGKILL");
            else assert.equal(code, 0);
          } finally {
            clearTimeout(timer);
          }
          client?.close();
        }
        t.after(async () => {
          await stop();
          await rm(directory, { recursive: true, force: true });
        });
        await command([
          "account-root-init",
          root,
          "--name",
          "synthetic-user-SDK",
          "--reset-at",
          "0",
        ]);
        const project = JSON.parse(
          await command(["account-root-projects", root]),
        ).projects[0].id;
        await command([
          "account-key-rotate",
          root,
          project,
          "--output",
          keyFile,
        ]);
        secrets.push(await readFile(keyFile, "utf8"), project);
        const prefix = [root, project, "--key-file", keyFile];
        const owner = JSON.parse(
          await command(
            ["account-user", ...prefix, "create", "synthetic_user"],
            "synthetic-private-user-password",
          ),
        ).user;
        const other = JSON.parse(
          await command(
            ["account-user", ...prefix, "create", "synthetic_other"],
            "synthetic-private-user-password",
          ),
        ).user;
        const data = join(root, "registry", project, "data"),
          privateStore = join(root, "private", project);
        await command([
          "sql",
          data,
          "CREATE TABLE owned(id INT PRIMARY KEY,owner BYTES,n INT)",
        ]);
        await command(["account-policy", ...prefix, "enable"]);
        await command(
          ["account-policy", ...prefix, "install", "owned", "--expected", "0"],
          JSON.stringify(OWN),
        );
        const closed = JSON.parse(
          await command(["account-admission", ...prefix, "enable-catalog"]),
        ).admission;
        await command([
          "account-admission",
          ...prefix,
          "open",
          "--expected",
          closed.revision,
        ]);
        if (compact) {
          await command(["compact", data]);
          await command(["compact", privateStore]);
        }
        await start();
        const row = (id, who, n) => [
          integer(id),
          { type: "bytes", value: [...Buffer.from(who.id, "hex")] },
          integer(n),
        ];
        const login = async (name) => {
          const pair = await client.signIn(
            name,
            "synthetic-private-user-password",
          );
          secrets.push(pair.access_token, pair.refresh_token);
          return pair;
        };
        let first = await login("synthetic_user");
        const second = await login("synthetic_other");
        assert.deepEqual(await client.me(first.access_token), owner);
        assert.deepEqual(await client.me(second.access_token), other);
        const huge = 9223372036854775807n,
          small = -9223372036854775808n;
        const receipt = await client.rowWrite(first.access_token, "owned", [
          { op: "insert", row: row(huge, owner, 1) },
          { op: "insert", row: row(small, owner, 2) },
        ]);
        assert.equal(receipt.changed, 2);
        await stop(true);
        await start();
        assert.deepEqual(
          await client.rowGet(first.access_token, "owned", integer(huge)),
          row(huge, owner, 1),
        );
        assert.equal(
          await client.rowGet(second.access_token, "owned", integer(huge)),
          null,
        );
        assert.deepEqual(
          await client.rowPage(second.access_token, "owned", 128),
          { rows: [], next: null },
        );
        const page = await client.rowPage(first.access_token, "owned", 1);
        assert.deepEqual(page.rows, [row(small, owner, 2)]);
        assert.deepEqual(
          (await client.rowPage(first.access_token, "owned", 1, page.next))
            .rows,
          [row(huge, owner, 1)],
        );
        await assert.rejects(
          client.rowWrite(first.access_token, "owned", [
            { op: "update", key: integer(huge), row: row(huge, owner, 9) },
            { op: "insert", row: row(3, other, 3) },
          ]),
          (e) =>
            e.code === "user_row_rejected" &&
            e.status === 403 &&
            e.outcome === "unknown",
        );
        assert.deepEqual(
          await client.rowGet(first.access_token, "owned", integer(huge)),
          row(huge, owner, 1),
        );
        assert.equal(
          await client.rowGet(second.access_token, "owned", integer(3)),
          null,
        );
        const previous = first;
        first = await client.refresh(first.refresh_token);
        secrets.push(first.access_token, first.refresh_token);
        await stop(true);
        await start();
        await denied(client.me(previous.access_token));
        await denied(client.refresh(previous.refresh_token));
        assert.deepEqual(await client.me(first.access_token), owner);
        let dispatched = 0,
          observed;
        const uncertain = new EmilyBaseUserClient({
          url,
          project,
          fetch: async (url, request) => {
            dispatched++;
            const r = await fetch(url, request);
            assert.equal(r.status, 200);
            observed = await r.json();
            throw new Error("synthetic-sensitive-lost-user-response");
          },
        });
        await assert.rejects(
          uncertain.rowWrite(first.access_token, "owned", [
            { op: "update", key: integer(huge), row: row(huge, owner, 7) },
          ]),
          (e) =>
            e.code === "transport_error" &&
            e.outcome === "unknown" &&
            !String(e).includes("synthetic-sensitive"),
        );
        assert.equal(dispatched, 1);
        assert.equal(observed.changed, 1);
        const lostRefresh = first.refresh_token;
        await assert.rejects(
          uncertain.refresh(lostRefresh),
          (e) => e.code === "transport_error" && e.outcome === "unknown",
        );
        assert.equal(dispatched, 2);
        // Only this test adapter observed the reply; the SDK received no pair.
        // Keep it explicitly to continue the fixture, never as an SDK recovery.
        first = observed;
        secrets.push(first.access_token, first.refresh_token);
        uncertain.close();
        await denied(client.refresh(lostRefresh));
        assert.deepEqual(
          await client.rowGet(first.access_token, "owned", integer(huge)),
          row(huge, owner, 7),
        );
        await stop();
        const bundle = join(directory, "copy.bundle"),
          copy = join(directory, "copy");
        await command(["account-root-backup", root, bundle]);
        await command([
          "account-bundle-restore",
          bundle,
          copy,
          "--reset-at",
          "0",
        ]);
        await start(copy);
        await denied(client.me(first.access_token));
        await stop();
        const copyPrefix = [copy, project, "--key-file", keyFile];
        const copyClosed = JSON.parse(
          await command(["account-admission", ...copyPrefix, "status"]),
        ).admission;
        assert.equal(copyClosed.enabled, false);
        await command([
          "account-admission",
          ...copyPrefix,
          "open",
          "--expected",
          copyClosed.revision,
        ]);
        await start(copy);
        await denied(client.me(first.access_token));
        const fresh = await login("synthetic_user");
        assert.deepEqual(
          await client.rowGet(fresh.access_token, "owned", integer(huge)),
          row(huge, owner, 7),
        );
        await stop();
        await start();
        assert.deepEqual(await client.me(first.access_token), owner);
        await client.logout(first.refresh_token);
        await stop(true);
        await start();
        await denied(client.me(first.access_token));
        await denied(client.refresh(first.refresh_token));
        assert.deepEqual(await client.me(second.access_token), other);
        await stop();
        await command([
          "account-user",
          ...prefix,
          "disable",
          "synthetic_other",
        ]);
        await start();
        await denied(client.me(second.access_token));
        await stop();
        await command(["account-user", ...prefix, "enable", "synthetic_other"]);
        await start();
        await denied(client.me(second.access_token));
        const reenabled = await login("synthetic_other");
        assert.equal((await client.me(reenabled.access_token)).disabled, false);
        await stop();
        for (const secret of secrets)
          assert(
            !logs.join("\n").includes(secret),
            "private value absent from actual server logs",
          );
      });
  },
);
