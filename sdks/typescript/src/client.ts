import * as migrations from "./migrations.js";
import * as rows from "./rows.js";
import * as decode from "./decode.js";
import { readBody } from "./response.js";
import { EmilyBaseError } from "./types.js";
import type {
  MigrationApplied,
  MigrationDefinition,
  MigrationReceipt,
  BatchChanged,
  RowChanged,
  RowKey,
  RowPage,
  RowValue,
  RowWrite,
  ClientOptions,
  Outcome,
  Plan,
  Report,
  RequestOptions,
  Status,
  Value,
} from "./types.js";

const encoder = new TextEncoder();
const MAX_RESPONSE_BYTES = 64 * 1024 * 1024;
const SAFE_REFUSALS = new Map([
  ["access_denied", 401],
  ["invalid_json", 400],
  ["invalid_project_request", 400],
  ["query_rejected", 400],
  ["table_rejected", 400],
  ["migration_rejected", 400],
  ["body_timeout", 408],
  ["body_limit", 413],
  ["json_required", 415],
  ["rate_limit", 429],
  ["workers_busy", 503],
]);
const ERROR_CODES = new Set([
  ...SAFE_REFUSALS.keys(),
  "transaction_outcome_requires_inspection",
  "publication_outcome_requires_inspection",
  "storage_unavailable",
  "migration_history_invalid",
  "limiter_unavailable",
  "registry_unavailable",
  "worker_failed",
  "request_scope",
  "peer_unavailable",
]);
function input(): never {
  throw new EmilyBaseError("invalid_input", "not_started");
}
function key(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value))
    return input();
  return value;
}
function payload(sql: string, parameters: readonly Value[]): string {
  if (
    typeof sql !== "string" ||
    encoder.encode(sql).length > 16384 ||
    !Array.isArray(parameters) ||
    parameters.length > 256
  )
    return input();
  let values: Value[];
  try {
    values = parameters.map(decode.value);
  } catch {
    return input();
  }
  const body = JSON.stringify({ sql, parameters: values });
  if (encoder.encode(body).length > 65536) return input();
  return body;
}
/** Project scope only. No implicit retries, credential persistence or administrator APIs. */
export class EmilyBaseClient {
  #url: string;
  #project: string;
  #key: string;
  #fetch: typeof globalThis.fetch;
  constructor(options: ClientOptions) {
    let url: URL;
    try {
      url = new URL(options.url);
    } catch {
      input();
    }
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.pathname !== "/"
    )
      input();
    if (
      typeof options.project !== "string" ||
      !/^[0-9a-f]{32}$/.test(options.project)
    )
      input();
    this.#url = url.origin;
    this.#project = options.project;
    this.#key = key(options.apiKey);
    const fetch = options.fetch ?? globalThis.fetch;
    if (typeof fetch !== "function") input();
    this.#fetch = fetch.bind(globalThis);
  }
  setKey(apiKey: string): void {
    if (!this.#key) throw new EmilyBaseError("client_closed", "not_started");
    this.#key = key(apiKey);
  }
  close(): void {
    this.#key = "";
  }
  toJSON(): object {
    return { project: this.#project, closed: !this.#key };
  }
  toString(): string {
    return "[EmilyBaseClient]";
  }
  sql(
    sql: string,
    parameters: readonly Value[] = [],
    options: RequestOptions = {},
  ): Promise<Report> {
    return this.#request(
      "sql",
      "POST",
      payload(sql, parameters),
      decode.report,
      options,
    );
  }
  explain(
    sql: string,
    parameters: readonly Value[] = [],
    options: RequestOptions = {},
  ): Promise<Plan> {
    return this.#request(
      "explain",
      "POST",
      payload(sql, parameters),
      decode.plan,
      options,
    );
  }
  status(options: RequestOptions = {}): Promise<Status> {
    return this.#request("status", "GET", undefined, decode.status, options);
  }
  rowGet(
    table: string,
    key: RowKey,
    options: RequestOptions = {},
  ): Promise<RowValue[] | null> {
    return this.#row(
      "get",
      () => ({ table: rows.table(table), key: rows.key(key) }),
      rows.found,
      options,
    );
  }
  rowPage(
    table: string,
    limit: number,
    after: RowKey | null = null,
    options: RequestOptions = {},
  ): Promise<RowPage> {
    return this.#row(
      "page",
      () => ({
        table: rows.table(table),
        limit: rows.limit(limit),
        after: after === null ? null : rows.key(after),
      }),
      (value) => rows.page(value, limit),
      options,
    );
  }
  rowInsert(
    table: string,
    row: readonly RowValue[],
    options: RequestOptions = {},
  ): Promise<RowChanged> {
    return this.#row(
      "insert",
      () => ({ table: rows.table(table), row: rows.row(row) }),
      rows.changed,
      options,
    );
  }
  rowUpdate(
    table: string,
    key: RowKey,
    row: readonly RowValue[],
    options: RequestOptions = {},
  ): Promise<RowChanged> {
    return this.#row(
      "update",
      () => ({
        table: rows.table(table),
        key: rows.key(key),
        row: rows.row(row),
      }),
      rows.changed,
      options,
    );
  }
  rowDelete(
    table: string,
    key: RowKey,
    options: RequestOptions = {},
  ): Promise<RowChanged> {
    return this.#row(
      "delete",
      () => ({ table: rows.table(table), key: rows.key(key) }),
      rows.changed,
      options,
    );
  }
  rowBatch(
    table: string,
    operations: readonly RowWrite[],
    options: RequestOptions = {},
  ): Promise<BatchChanged> {
    return this.#row(
      "batch",
      () => ({ table: rows.table(table), operations: rows.writes(operations) }),
      rows.batch,
      options,
    );
  }
  migrationList(options: RequestOptions = {}): Promise<MigrationReceipt[]> {
    return this.#request(
      "migrations",
      "GET",
      undefined,
      migrations.inventory,
      options,
      65536,
    );
  }
  migrationApply(
    definition: MigrationDefinition,
    options: RequestOptions = {},
  ): Promise<MigrationApplied> {
    let snapshot: MigrationDefinition;
    let body: string;
    try {
      snapshot = migrations.definition(definition);
      body = migrations.payload(snapshot);
    } catch {
      return input();
    }
    return this.#request(
      "migrations/apply",
      "POST",
      body,
      (value) => migrations.applied(value, snapshot),
      options,
      65536,
    );
  }
  #row<T>(
    operation: string,
    data: () => unknown,
    parse: (value: unknown) => T,
    options: RequestOptions,
  ): Promise<T> {
    let body: string;
    try {
      body = rows.payload(data());
    } catch {
      return input();
    }
    return this.#request(
      "tables/rows/" + operation,
      "POST",
      body,
      parse,
      options,
      65536,
    );
  }
  async #request<T>(
    route: string,
    method: string,
    data: string | undefined,
    parse: (input: unknown) => T,
    options: RequestOptions,
    maximum = MAX_RESPONSE_BYTES,
  ): Promise<T> {
    if (!this.#key) throw new EmilyBaseError("client_closed", "not_started");
    if (
      options.timeoutMs !== undefined &&
      (!Number.isSafeInteger(options.timeoutMs) ||
        options.timeoutMs < 1 ||
        options.timeoutMs > 300000)
    )
      return input();
    if (options.signal?.aborted)
      throw new EmilyBaseError("cancelled", "not_started");
    const controller = new AbortController();
    const cancel = () => controller.abort();
    options.signal?.addEventListener("abort", cancel, { once: true });
    const timeout =
      options.timeoutMs === undefined
        ? undefined
        : setTimeout(cancel, options.timeoutMs);
    try {
      const headers = new Headers({
        authorization: `Bearer ${this.#key}`,
        accept: "application/json",
      });
      if (data !== undefined) headers.set("content-type", "application/json");
      const request: RequestInit = {
        method,
        headers,
        signal: controller.signal,
        redirect: "error",
        cache: "no-store",
        credentials: "omit",
        referrerPolicy: "no-referrer",
      };
      if (data !== undefined) request.body = data;
      const response = await this.#fetch(
        `${this.#url}/v1/projects/${this.#project}/${route}`,
        request,
      );
      const json = await readBody(response, maximum);
      if (!response.ok) {
        const code =
          json !== null &&
          typeof json === "object" &&
          Object.keys(json).length === 1 &&
          Object.hasOwn(json, "code")
            ? (json as { code: unknown }).code
            : undefined;
        // Known fixed codes only; never echo arbitrary error text from a peer.
        const known = typeof code === "string" && ERROR_CODES.has(code);
        const outcome: Outcome =
          known && SAFE_REFUSALS.get(code) === response.status
            ? "not_committed"
            : "unknown";
        throw new EmilyBaseError(
          known ? code : "http_error",
          outcome,
          response.status,
        );
      }
      try {
        return parse(json);
      } catch {
        throw new EmilyBaseError("protocol_error", "unknown", response.status);
      }
    } catch (error) {
      if (error instanceof EmilyBaseError) throw error;
      throw new EmilyBaseError(
        controller.signal.aborted ? "cancelled" : "transport_error",
        "unknown",
      );
    } finally {
      if (timeout !== undefined) clearTimeout(timeout);
      options.signal?.removeEventListener("abort", cancel);
    }
  }
}
