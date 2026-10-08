import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { randomBytes, createHash } from "node:crypto";
import { EmilyBaseClient, EmilyBaseError } from "../dist/index.js";

test(
  "SDK communicates with the original HTTP/WAL engine",
  { timeout: 30000 },
  async (t) => {
    const directory = await mkdtemp(join(tmpdir(), "emilybase-sdk-"));
    const external = process.env.EMILYBASE_TEST_URL;
    const master = external
      ? process.env.EMILYBASE_TEST_MASTER_KEY
      : randomBytes(32).toString("hex");
    assert(
      master && /^[0-9a-f]{64}$/.test(master),
      "synthetic master configuration",
    );
    let child,
      url = external;
    const logs = [];
    async function start() {
      const binary =
        process.env.EMILYBASE_SERVER_BIN ??
        fileURLToPath(
          new URL("../../../target/debug/emilybase-server", import.meta.url),
        );
      child = spawn(binary, [], {
        env: {
          ...process.env,
          EMILYBASE_MASTER_KEY: master,
          EMILYBASE_MASTER_KEY_FILE: undefined,
          EMILYBASE_ACCOUNT_ROOT: undefined,
          EMILYBASE_DATA_DIR: join(directory, "projects"),
          EMILYBASE_LISTEN: "127.0.0.1:0",
        },
        stdio: ["ignore", "pipe", "pipe"],
      });
      const ready = new Promise((resolve, reject) => {
        child.once("error", () => reject(new Error("server startup failed")));
        child.once("exit", () =>
          reject(new Error("server exited during startup")),
        );
        createInterface({ input: child.stdout }).on("line", (line) => {
          logs.push(line);
          const record = JSON.parse(line);
          if (record.fields?.message === "experimental_server_listening")
            resolve(`http://${record.fields.address}`);
        });
      });
      let timer;
      try {
        url = await Promise.race([
          ready,
          new Promise((_, reject) => {
            timer = setTimeout(
              () => reject(new Error("server startup deadline")),
              10000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
    }
    async function stop() {
      if (!child || child.exitCode !== null || child.signalCode !== null)
        return;
      const exited = once(child, "exit");
      child.kill("SIGTERM");
      const timer = setTimeout(() => child.kill("SIGKILL"), 10000);
      try {
        const [code] = await exited;
        assert.equal(code, 0, "clean server shutdown");
      } finally {
        clearTimeout(timer);
      }
    }
    t.after(async () => {
      await stop();
      await rm(directory, { recursive: true, force: true });
    });
    if (!external) await start();
    async function admin(path, method, payload) {
      const options = {
        method,
        headers: {
          authorization: `Bearer ${master}`,
          "content-type": "application/json",
        },
      };
      if (payload !== undefined) options.body = JSON.stringify(payload);
      const response = await fetch(url + path, options);
      assert(response.ok, `administrator status ${response.status}`);
      return response.json();
    }
    const first = await admin("/v1/projects", "POST", { name: "sdk-first" });
    const second = await admin("/v1/projects", "POST", { name: "sdk-second" });
    let key = first.api_key;
    let client = new EmilyBaseClient({
      url,
      project: first.project.id,
      apiKey: key,
    });
    const other = new EmilyBaseClient({
      url,
      project: second.project.id,
      apiKey: second.api_key,
    });
    const denied = (promise) =>
      assert.rejects(
        promise,
        (error) =>
          error instanceof EmilyBaseError &&
          error.code === "access_denied" &&
          error.outcome === "not_committed",
      );
    const literal = "'); DROP TABLE t; -- Привет";
    await t.test(
      "DDL parameter literals all types and nulls round-trip",
      async () => {
        const report = await client.sql(
          "CREATE TABLE t(id INTEGER PRIMARY KEY,flag BOOLEAN,score FLOAT,label TEXT,payload BYTES); INSERT INTO t VALUES (1,$1,$2,$3,$4),(2,NULL,NULL,NULL,NULL); SELECT * FROM t ORDER BY id",
          [
            { type: "boolean", value: true },
            { type: "float", value: 3.5 },
            { type: "text", value: literal },
            { type: "bytes", value: [0, 255] },
          ],
        );
        assert.equal(report.transaction, 2);
        assert.equal(report.committed, true);
        assert.deepEqual(report.results[2].rows[0], [
          { type: "integer", value: 1 },
          { type: "boolean", value: true },
          { type: "float", value: 3.5 },
          { type: "text", value: literal },
          { type: "bytes", value: [0, 255] },
        ]);
        assert.deepEqual(
          report.results[2].rows[1].slice(1),
          Array.from({ length: 4 }, () => ({ type: "null" })),
        );
        assert.equal(
          (
            await client.explain("SELECT * FROM t WHERE id=$1", [
              { type: "integer", value: 1 },
            ])
          ).access,
          "primary_key",
        );
        assert.equal(
          (await client.explain("SELECT * FROM t WHERE id>=1 AND id<3")).access,
          "primary_range",
        );
        const joinSql =
          "SELECT a.id,b.id FROM t AS a JOIN t AS b ON a.id=b.id ORDER BY a.id";
        assert.equal((await client.explain(joinSql)).access, "primary_join");
        assert.deepEqual((await client.sql(joinSql)).results[0].rows, [
          [
            { type: "integer", value: 1 },
            { type: "integer", value: 1 },
          ],
          [
            { type: "integer", value: 2 },
            { type: "integer", value: 2 },
          ],
        ]);
        const sortedJoin =
          "SELECT a.id FROM t AS a JOIN t AS b ON a.id=b.id ORDER BY b.id DESC LIMIT $1";
        assert.equal(
          (await client.explain(sortedJoin, [{ type: "integer", value: 1 }]))
            .access,
          "primary_join",
        );
        assert.deepEqual(
          (await client.sql(sortedJoin, [{ type: "integer", value: 1 }]))
            .results[0].rows,
          [[{ type: "integer", value: 2 }]],
        );
        const sortedTable =
          "SELECT id FROM t ORDER BY label DESC NULLS FIRST LIMIT $1";
        assert.equal(
          (await client.explain(sortedTable, [{ type: "integer", value: 1 }]))
            .access,
          "scan",
        );
        assert.deepEqual(
          (await client.sql(sortedTable, [{ type: "integer", value: 1 }]))
            .results[0].rows,
          [[{ type: "integer", value: 2 }]],
        );
        assert.deepEqual(
          (
            await client.sql(
              "SELECT id FROM t WHERE id>=1 AND id<3 ORDER BY id",
            )
          ).results[0].rows,
          [[{ type: "integer", value: 1 }], [{ type: "integer", value: 2 }]],
        );
      },
    );
    await t.test(
      "rejected scripts and rollback preserve committed status",
      async () => {
        await assert.rejects(
          client.sql(
            "INSERT INTO t VALUES(100,NULL,NULL,NULL,NULL); INSERT INTO t VALUES(1,NULL,NULL,NULL,NULL)",
          ),
          (error) =>
            error.code === "query_rejected" &&
            error.outcome === "not_committed",
        );
        assert.deepEqual(await client.status(), {
          transaction: 2,
          tables: 1,
          rows: 2,
        });
        const report = await client.sql(
          "BEGIN; INSERT INTO t VALUES(100,NULL,NULL,NULL,NULL); SELECT id FROM t WHERE id=100; ROLLBACK",
        );
        assert.equal(report.committed, false);
        assert.equal(report.results[1].rows[0][0].value, 100);
        assert.deepEqual(await client.status(), {
          transaction: 2,
          tables: 1,
          rows: 2,
        });
      },
    );
    await t.test(
      "UPDATE DELETE filters and ordering execute through scoped SDK",
      async () => {
        const report = await client.sql(
          "UPDATE t SET label=$1 WHERE id=1; DELETE FROM t WHERE id=2; SELECT id,label FROM t ORDER BY id LIMIT 1",
          [{ type: "text", value: "updated" }],
        );
        assert.equal(report.transaction, 3);
        assert.equal(report.results[0].affected, 1);
        assert.equal(report.results[1].affected, 1);
        assert.deepEqual(report.results[2].rows, [
          [
            { type: "integer", value: 1 },
            { type: "text", value: "updated" },
          ],
        ]);
      },
    );
    await t.test(
      "project scope and atomic rotation reject old keys",
      async () => {
        assert.deepEqual(await other.status(), {
          transaction: 1,
          tables: 0,
          rows: 0,
        });
        await denied(
          new EmilyBaseClient({
            url,
            project: second.project.id,
            apiKey: key,
          }).status(),
        );
        await denied(
          new EmilyBaseClient({
            url,
            project: first.project.id,
            apiKey: master,
          }).status(),
        );
        const rotated = await admin(
          `/v1/projects/${first.project.id}/keys/rotate`,
          "POST",
        );
        assert(key !== rotated.api_key, "rotation changes key");
        assert.equal(rotated.project.key_epoch, 2);
        await denied(client.status());
        key = rotated.api_key;
        client.setKey(key);
        assert.deepEqual(await client.status(), {
          transaction: 3,
          tables: 1,
          rows: 1,
        });
      },
    );
    await t.test(
      "unsafe i64 response is unknown outcome even when engine committed",
      async () => {
        await assert.rejects(
          client.sql(
            "CREATE TABLE huge(id INTEGER PRIMARY KEY); INSERT INTO huge VALUES(9223372036854775807); SELECT id FROM huge",
          ),
          (error) =>
            error.code === "protocol_error" && error.outcome === "unknown",
        );
        assert.deepEqual(await client.status(), {
          transaction: 4,
          tables: 2,
          rows: 2,
        });
      },
    );
    await t.test(
      "server restart preserves credentials SQL rows and transaction IDs",
      {
        skip: external
          ? "external lifecycle belongs to deployment probe"
          : false,
      },
      async () => {
        await stop();
        await start();
        client = new EmilyBaseClient({
          url,
          project: first.project.id,
          apiKey: key,
        });
        assert.deepEqual(await client.status(), {
          transaction: 4,
          tables: 2,
          rows: 2,
        });
        assert.equal(
          (await client.sql("SELECT label FROM t WHERE id=1")).results[0]
            .rows[0][0].value,
          "updated",
        );
        const text = logs.join("\n");
        for (const privateValue of [
          master,
          key,
          first.api_key,
          second.api_key,
          first.project.id,
          second.project.id,
          literal,
          "INSERT INTO",
        ])
          assert(
            !text.includes(privateValue),
            "private value absent from server logs",
          );
      },
    );
    await t.test(
      "exact row SDK batch rollback and acknowledged kill recover every bit",
      async () => {
        await client.sql(
          "CREATE TABLE wire_rows(id INTEGER PRIMARY KEY,score FLOAT,payload BYTES,label TEXT)",
        );
        const huge = { type: "integer", value: "9223372036854775807" };
        const original = [
          huge,
          { type: "float_bits", value: "8000000000000000" },
          { type: "bytes", value: [0, 255] },
          { type: "text", value: "synthetic-row-SDK'界" },
        ];
        const inserted = await client.rowInsert("wire_rows", original);
        assert.deepEqual(inserted.key, huge);
        assert.equal(typeof inserted.transaction, "string");
        assert.deepEqual(await client.rowGet("wire_rows", huge), original);
        const small = { type: "integer", value: "-9223372036854775808" };
        const replacement = [
          huge,
          { type: "float_bits", value: "7fefffffffffffff" },
          { type: "bytes", value: [7] },
          { type: "null" },
        ];
        const changed = await client.rowBatch("wire_rows", [
          { op: "update", key: huge, row: replacement },
          {
            op: "insert",
            row: [
              small,
              { type: "float_bits", value: "0000000000000001" },
              { type: "null" },
              { type: "null" },
            ],
          },
        ]);
        assert.equal(changed.changed, 2);
        assert(BigInt(changed.transaction) > BigInt(inserted.transaction));
        await assert.rejects(
          client.rowBatch("wire_rows", [
            { op: "delete", key: huge },
            { op: "delete", key: huge },
          ]),
          (error) =>
            error.code === "table_rejected" &&
            error.outcome === "not_committed",
        );
        const firstPage = await client.rowPage("wire_rows", 1);
        assert.deepEqual(firstPage.next, small);
        assert.deepEqual(
          (await client.rowPage("wire_rows", 1, firstPage.next)).rows,
          [replacement],
        );
        if (!external) {
          const exited = once(child, "exit");
          child.kill("SIGKILL");
          await exited;
          await start();
          client = new EmilyBaseClient({
            url,
            project: first.project.id,
            apiKey: key,
          });
        }
        assert.deepEqual(await client.rowGet("wire_rows", huge), replacement);
        await client.rowUpdate("wire_rows", huge, original);
        assert.deepEqual(await client.rowGet("wire_rows", huge), original);
        await client.rowDelete("wire_rows", huge);
        assert.equal(await client.rowGet("wire_rows", huge), null);
        await client.sql("DROP TABLE wire_rows");
        for (const secret of ["wire_rows", "synthetic-row-SDK", master, key])
          assert(!logs.join("\n").includes(secret));
      },
    );
    await t.test(
      "migration SDK exact retries, copied schema and ACK kills retain receipt authority",
      async () => {
        const migrationProject = first.project.id;
        const secondProject = second.project.id,
          secondKey = second.api_key;
        const definitions = [
          {
            version: 1,
            label: "sdk-initial",
            sql: "CREATE TABLE migrate_source(id INT PRIMARY KEY,v TEXT); INSERT INTO migrate_source VALUES(1,'synthetic-SDK-migration')",
          },
          {
            version: 2,
            label: "sdk-rebuild",
            sql: "CREATE TABLE migrate_copy(id INT PRIMARY KEY,v TEXT,note TEXT); INSERT INTO migrate_copy(id,v) SELECT * FROM migrate_source; DROP TABLE migrate_source; CREATE TABLE migrate_source(id INT PRIMARY KEY,v TEXT,note TEXT); INSERT INTO migrate_source SELECT * FROM migrate_copy; DROP TABLE migrate_copy",
          },
        ];
        const digest = (definition) => {
          const label = Buffer.from(definition.label),
            sql = Buffer.from(definition.sql);
          const number = (n) => {
            const b = Buffer.alloc(4);
            b.writeUInt32BE(n);
            return b;
          };
          return createHash("sha256")
            .update(Buffer.from("emilybase-migration-v1\0"))
            .update(number(definition.version))
            .update(number(label.length))
            .update(label)
            .update(number(sql.length))
            .update(sql)
            .digest("hex");
        };
        assert.deepEqual(await client.migrationList(), []);
        const receipts = [];
        for (const definition of definitions) {
          const [first, second] = await Promise.all([
            client.migrationApply(definition),
            client.migrationApply(definition),
          ]);
          assert.notEqual(first.already_applied, second.already_applied);
          assert.deepEqual(first.receipt, second.receipt);
          assert.equal(first.receipt.sha256, digest(definition));
          receipts.push(first.receipt);
          if (!external) {
            const exited = once(child, "exit");
            child.kill("SIGKILL");
            await exited;
            await start();
            client = new EmilyBaseClient({
              url,
              project: migrationProject,
              apiKey: key,
            });
          }
          assert.deepEqual(await client.migrationApply(definition), {
            receipt: first.receipt,
            already_applied: true,
          });
          assert.deepEqual(await client.migrationList(), receipts);
          const sibling = new EmilyBaseClient({
            url,
            project: secondProject,
            apiKey: secondKey,
          });
          try {
            assert.deepEqual(await sibling.migrationList(), []);
          } finally {
            sibling.close();
          }
        }
        assert.deepEqual(
          await client.rowGet("migrate_source", {
            type: "integer",
            value: "1",
          }),
          [
            { type: "integer", value: "1" },
            { type: "text", value: "synthetic-SDK-migration" },
            { type: "null" },
          ],
        );
        await assert.rejects(
          client.migrationApply({
            ...definitions[1],
            sql: definitions[1].sql + ";",
          }),
          (e) =>
            e.code === "migration_rejected" && e.outcome === "not_committed",
        );
        await assert.rejects(
          client.migrationApply({
            version: 3,
            label: "sdk-late-failure",
            sql: "CREATE TABLE never_committed(id INT PRIMARY KEY); INSERT INTO migrate_source(id) VALUES(1)",
          }),
          (e) =>
            e.code === "migration_rejected" && e.outcome === "not_committed",
        );
        assert.deepEqual(await client.migrationList(), receipts);
        const uncertainDefinition = {
          version: 3,
          label: "sdk-lost-response",
          sql: "UPDATE migrate_source SET note='synthetic-lost-response'",
        };
        let dispatched = 0,
          observed;
        const uncertain = new EmilyBaseClient({
          url,
          project: migrationProject,
          apiKey: key,
          fetch: async (url, request) => {
            dispatched++;
            const response = await fetch(url, request);
            assert.equal(response.status, 200);
            observed = await response.json();
            throw new Error("synthetic-sensitive-lost-response");
          },
        });
        await assert.rejects(
          uncertain.migrationApply(uncertainDefinition),
          (e) =>
            e.code === "transport_error" &&
            e.outcome === "unknown" &&
            !String(e).includes("synthetic-sensitive-lost-response"),
        );
        uncertain.close();
        assert.equal(dispatched, 1);
        assert.equal(observed.already_applied, false);
        assert.equal(observed.receipt.sha256, digest(uncertainDefinition));
        receipts.push(observed.receipt);
        if (!external) {
          const exited = once(child, "exit");
          child.kill("SIGKILL");
          await exited;
          await start();
          client = new EmilyBaseClient({
            url,
            project: migrationProject,
            apiKey: key,
          });
        }
        assert.deepEqual(await client.migrationApply(uncertainDefinition), {
          receipt: observed.receipt,
          already_applied: true,
        });
        assert.deepEqual(await client.migrationList(), receipts);
        assert.equal(
          (
            await client.rowGet("migrate_source", {
              type: "integer",
              value: "1",
            })
          )[2].value,
          "synthetic-lost-response",
        );
        const wrong = new EmilyBaseClient({
          url,
          project: migrationProject,
          apiKey: second.api_key,
        });
        await denied(wrong.migrationList());
        await denied(wrong.migrationApply(definitions[0]));
        wrong.close();
        const rotated = await admin(
          `/v1/projects/${first.project.id}/keys/rotate`,
          "POST",
        );
        await denied(client.migrationList());
        await denied(client.migrationApply(definitions[0]));
        key = rotated.api_key;
        client.setKey(key);
        assert.deepEqual(await client.migrationList(), receipts);
        assert.deepEqual(await client.migrationApply(definitions[0]), {
          receipt: receipts[0],
          already_applied: true,
        });
        for (const secret of [
          master,
          key,
          definitions[0].sql,
          definitions[1].sql,
          "synthetic-SDK-migration",
          "synthetic-lost-response",
          uncertainDefinition.sql,
        ])
          assert(!logs.join("\n").includes(secret));
      },
    );
    client.close();
    other.close();
  },
);
