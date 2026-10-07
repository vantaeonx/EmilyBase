import type { Plan, Report, ResultSet, Status, Value } from "./types.js";
const encoder = new TextEncoder();
export class InvalidProtocol extends Error {}
function invalid(): never {
  throw new InvalidProtocol();
}
function record(value: unknown, keys: string[]): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    return invalid();
  const result = value as Record<string, unknown>;
  if (
    Object.keys(result).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(result, key))
  )
    return invalid();
  return result;
}
export function unsigned(
  value: unknown,
  maximum = Number.MAX_SAFE_INTEGER,
): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > maximum
  )
    return invalid();
  return value;
}
function text(value: unknown, maximum: number): string {
  if (typeof value !== "string" || encoder.encode(value).length > maximum)
    return invalid();
  return value;
}
function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") return invalid();
  return value;
}
function array(value: unknown, maximum: number): unknown[] {
  if (!Array.isArray(value) || value.length > maximum) return invalid();
  return value;
}
export function value(input: unknown): Value {
  if (input === null || typeof input !== "object") return invalid();
  const kind = (input as Record<string, unknown>).type;
  const object = record(input, kind === "null" ? ["type"] : ["type", "value"]);
  switch (kind) {
    case "null":
      return { type: "null" };
    case "boolean":
      return { type: kind, value: boolean(object.value) };
    case "text":
      return { type: kind, value: text(object.value, 3072) };
    case "bytes":
      return {
        type: kind,
        value: array(object.value, 3072).map((byte) => unsigned(byte, 255)),
      };
    case "integer": {
      const number = object.value;
      if (typeof number !== "number" || !Number.isSafeInteger(number))
        return invalid();
      return { type: kind, value: number };
    }
    case "float": {
      const number = object.value;
      if (typeof number !== "number" || !Number.isFinite(number))
        return invalid();
      return { type: kind, value: number };
    }
    default:
      return invalid();
  }
}
function result(input: unknown): ResultSet {
  const object = record(input, ["columns", "rows", "affected"]);
  const columns = array(object.columns, 128).map((column) => text(column, 127));
  const rows = array(object.rows, 10000).map((input) => {
    const row = array(input, 128);
    if (row.length !== columns.length) return invalid();
    return row.map(value);
  });
  return { columns, rows, affected: unsigned(object.affected, 10000) };
}
export function report(input: unknown): Report {
  const object = record(input, ["transaction", "committed", "results"]);
  return {
    transaction: unsigned(object.transaction),
    committed: boolean(object.committed),
    results: array(object.results, 64).map(result),
  };
}
export function status(input: unknown): Status {
  const object = record(input, ["transaction", "tables", "rows"]);
  return {
    transaction: unsigned(object.transaction),
    tables: unsigned(object.tables, 128),
    rows: unsigned(object.rows, 10000),
  };
}
export function plan(input: unknown): Plan {
  const object = record(input, [
    "access",
    "table",
    "joined_table",
    "sorted",
    "limit",
  ]);
  const access = object.access;
  if (
    access !== "primary_key" &&
    access !== "primary_range" &&
    access !== "primary_join" &&
    access !== "scan" &&
    access !== "bounded_nested_loop"
  )
    return invalid();
  return {
    access,
    table: text(object.table, 63),
    joined_table:
      object.joined_table === null ? null : text(object.joined_table, 63),
    sorted: boolean(object.sorted),
    limit: unsigned(object.limit, 10000),
  };
}
