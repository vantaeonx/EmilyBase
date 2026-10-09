import * as wire from "./user-wire.js";
import * as rows from "./rows.js";
import { readBody } from "./response.js";
import { EmilyBaseError } from "./types.js";
import type {
  UserClientOptions,
  UserSession,
  UserInfo,
  RequestOptions,
  RowKey,
  RowValue,
  RowPage,
  RowWrite,
  BatchChanged,
} from "./types.js";

const ERROR_CODES = new Set([
  "access_denied",
  "invalid_json",
  "invalid_account_request",
  "account_capacity",
  "password_workers_busy",
  "trusted_clock_unavailable",
  "session_outcome_requires_inspection",
  "user_row_rejected",
  "user_row_outcome_requires_inspection",
  "policy_binding_invalid",
  "policy_catalog_disabled",
  "policy_catalog_invalid",
  "body_timeout",
  "body_limit",
  "json_required",
  "rate_limit",
  "account_rate_limit",
  "workers_busy",
  "limiter_unavailable",
  "registry_unavailable",
  "worker_failed",
  "peer_unavailable",
  "storage_unavailable",
]);
function input(): never {
  throw new EmilyBaseError("invalid_input", "not_started");
}

/** Explicit caller-owned tokens. No service keys, credential storage or automatic retries. */
export class EmilyBaseUserClient {
  #url: string;
  #project: string;
  #fetch: typeof globalThis.fetch;
  #closed = false;
  constructor(options: UserClientOptions) {
    if (
      !options ||
      typeof options !== "object" ||
      Array.isArray(options) ||
      Object.keys(options).some((k) => !["url", "project", "fetch"].includes(k))
    )
      input();
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
    const fetch = options.fetch ?? globalThis.fetch;
    if (typeof fetch !== "function") input();
    this.#url = url.origin;
    this.#project = options.project;
    this.#fetch = fetch.bind(globalThis);
  }
  close(): void {
    this.#closed = true;
  }
  toJSON(): object {
    return { project: this.#project, closed: this.#closed };
  }
  toString(): string {
    return "[EmilyBaseUserClient]";
  }
  signIn(
    login: string,
    password: string,
    options: RequestOptions = {},
  ): Promise<UserSession> {
    return this.#send(
      "sign-in",
      undefined,
      () => ({ login: wire.login(login), password: wire.password(password) }),
      wire.session,
      options,
    );
  }
  refresh(
    refreshToken: string,
    options: RequestOptions = {},
  ): Promise<UserSession> {
    return this.#send(
      "refresh",
      undefined,
      () => ({ refresh_token: wire.token(refreshToken, "refresh") }),
      wire.session,
      options,
    );
  }
  logout(
    refreshToken: string,
    options: RequestOptions = {},
  ): Promise<{ logged_out: true }> {
    return this.#send(
      "logout",
      undefined,
      () => ({ refresh_token: wire.token(refreshToken, "refresh") }),
      wire.logout,
      options,
    );
  }
  me(accessToken: string, options: RequestOptions = {}): Promise<UserInfo> {
    return this.#send("me", accessToken, () => ({}), wire.user, options);
  }
  changePassword(
    accessToken: string,
    currentPassword: string,
    replacementPassword: string,
    options: RequestOptions = {},
  ): Promise<UserInfo> {
    return this.#send(
      "password",
      accessToken,
      () => ({
        current_password: wire.password(currentPassword),
        replacement_password: wire.password(replacementPassword),
      }),
      wire.user,
      options,
    );
  }
  rowGet(
    accessToken: string,
    table: string,
    key: RowKey,
    options: RequestOptions = {},
  ): Promise<RowValue[] | null> {
    return this.#send(
      "rows/get",
      accessToken,
      () => ({ table: rows.table(table), key: rows.key(key) }),
      rows.found,
      options,
      65536,
    );
  }
  rowPage(
    accessToken: string,
    table: string,
    limit: number,
    after: RowKey | null = null,
    options: RequestOptions = {},
  ): Promise<RowPage> {
    return this.#send(
      "rows/page",
      accessToken,
      () => ({
        table: rows.table(table),
        limit: rows.limit(limit),
        after: after === null ? null : rows.key(after),
      }),
      (value) => rows.page(value, limit),
      options,
      65536,
    );
  }
  rowWrite(
    accessToken: string,
    table: string,
    operations: readonly RowWrite[],
    options: RequestOptions = {},
  ): Promise<BatchChanged> {
    let count = 0;
    return this.#send(
      "rows/write",
      accessToken,
      () => {
        const packet = rows.writes(operations);
        count = packet.length;
        return { table: rows.table(table), operations: packet };
      },
      (value) => {
        const result = rows.batch(value);
        if (result.changed !== count) throw new Error("invalid count");
        return result;
      },
      options,
      65536,
    );
  }
  #send<T>(
    route: string,
    access: string | undefined,
    data: () => unknown,
    parse: (input: unknown) => T,
    options: RequestOptions,
    maximum = 4096,
  ): Promise<T> {
    if (this.#closed) throw new EmilyBaseError("client_closed", "not_started");
    let body: string;
    try {
      if (!["sign-in", "refresh", "logout"].includes(route))
        access = wire.token(access, "access");
      body = wire.payload(data(), maximum);
    } catch {
      return input();
    }
    return this.#request(route, access, body, parse, options, maximum);
  }
  async #request<T>(
    route: string,
    access: string | undefined,
    body: string,
    parse: (input: unknown) => T,
    options: RequestOptions,
    maximum: number,
  ): Promise<T> {
    if (
      !options ||
      typeof options !== "object" ||
      Array.isArray(options) ||
      (options.signal !== undefined && !(options.signal instanceof AbortSignal))
    )
      return input();
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
        accept: "application/json",
        "content-type": "application/json",
      });
      if (access !== undefined)
        headers.set("authorization", `Bearer ${access}`);
      const response = await this.#fetch(
        `${this.#url}/v1/projects/${this.#project}/user/${route}`,
        {
          method: "POST",
          headers,
          body,
          signal: controller.signal,
          redirect: "error",
          cache: "no-store",
          credentials: "omit",
          referrerPolicy: "no-referrer",
        },
      );
      const json = await readBody(response, maximum);
      if (response.status !== 200) {
        const code =
          json !== null &&
          typeof json === "object" &&
          !Array.isArray(json) &&
          Object.keys(json).length === 1 &&
          Object.hasOwn(json, "code")
            ? (json as { code: unknown }).code
            : undefined;
        // Private trusted time can commit even when a session/row request refuses.
        // Remote refusals never imply that every database WAL stayed unchanged.
        throw new EmilyBaseError(
          typeof code === "string" && ERROR_CODES.has(code)
            ? code
            : "http_error",
          "unknown",
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
