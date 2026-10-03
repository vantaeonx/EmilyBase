export type Value =
  | { type: "null" }
  | { type: "boolean"; value: boolean }
  | { type: "integer"; value: number }
  | { type: "float"; value: number }
  | { type: "text"; value: string }
  | { type: "bytes"; value: number[] };

export interface ResultSet {
  columns: string[];
  rows: Value[][];
  affected: number;
}
export interface Report {
  transaction: number;
  committed: boolean;
  results: ResultSet[];
}
export interface Status {
  transaction: number;
  tables: number;
  rows: number;
}
export interface Plan {
  access: "primary_key" | "scan" | "bounded_nested_loop";
  table: string;
  joined_table: string | null;
  sorted: boolean;
  limit: number;
}
export interface RequestOptions {
  signal?: AbortSignal;
  /** Cancellation after dispatch can leave a write with an unknown outcome. */
  timeoutMs?: number;
}
export interface ClientOptions {
  url: string;
  project: string;
  apiKey: string;
  fetch?: typeof globalThis.fetch;
}
export type Outcome = "not_started" | "not_committed" | "unknown";
export class EmilyBaseError extends Error {
  readonly code: string;
  readonly outcome: Outcome;
  readonly status: number | undefined;
  constructor(code: string, outcome: Outcome, status?: number) {
    // Never copy the SQL, header, response body or original transport error.
    super(`EmilyBase request failed: ${code}`);
    this.name = "EmilyBaseError";
    this.code = code;
    this.outcome = outcome;
    this.status = status;
  }
}
