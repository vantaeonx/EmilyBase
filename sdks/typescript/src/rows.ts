import * as decode from "./decode.js";
import type {
  BatchChanged,
  RowChanged,
  RowKey,
  RowPage,
  RowValue,
  RowWrite,
} from "./types.js";
const encoder = new TextEncoder();
function invalid(): never {
  throw new decode.InvalidProtocol();
}
function record(
  input: unknown,
  fields: readonly string[],
): Record<string, unknown> {
  if (input === null || typeof input !== "object" || Array.isArray(input))
    return invalid();
  const object = input as Record<string, unknown>;
  if (
    Object.keys(object).length !== fields.length ||
    fields.some((f) => !Object.hasOwn(object, f))
  )
    return invalid();
  return object;
}
export function table(input: unknown): string {
  if (typeof input !== "string" || !/^[A-Za-z_][A-Za-z0-9_]{0,62}$/.test(input))
    return invalid();
  return input;
}
function decimal(input: unknown, minimum: bigint, maximum: bigint): string {
  if (
    typeof input !== "string" ||
    input.length > 20 ||
    !/^(0|-?[1-9][0-9]*)$/.test(input)
  )
    return invalid();
  const value = BigInt(input);
  if (value < minimum || value > maximum) return invalid();
  return input;
}
function text(input: unknown): string {
  if (typeof input !== "string" || encoder.encode(input).length > 3072)
    return invalid();
  return input;
}
export function key(input: unknown): RowKey {
  const object = record(input, ["type", "value"]);
  if (object.type === "integer")
    return {
      type: "integer",
      value: decimal(object.value, -(1n << 63n), (1n << 63n) - 1n),
    };
  if (object.type === "text")
    return { type: "text", value: text(object.value) };
  return invalid();
}
export function value(input: unknown): RowValue {
  if (input === null || typeof input !== "object") return invalid();
  const kind = (input as Record<string, unknown>).type;
  if (kind === "integer" || kind === "text") return key(input);
  if (kind === "float_bits") {
    const object = record(input, ["type", "value"]);
    const bits = object.value;
    if (
      typeof bits !== "string" ||
      !/^[0-9a-f]{16}$/.test(bits) ||
      ((BigInt("0x" + bits) >> 52n) & 0x7ffn) === 0x7ffn
    )
      return invalid();
    return { type: kind, value: bits };
  }
  if (kind !== "null" && kind !== "boolean" && kind !== "bytes")
    return invalid();
  const decoded = decode.value(input);
  if (
    decoded.type === "null" ||
    decoded.type === "boolean" ||
    decoded.type === "bytes"
  )
    return decoded;
  return invalid();
}
export function row(input: unknown): RowValue[] {
  if (!Array.isArray(input) || input.length < 1 || input.length > 64)
    return invalid();
  return input.map(value);
}
export function limit(input: unknown): number {
  const n = decode.unsigned(input, 128);
  if (n < 1) return invalid();
  return n;
}
export function writes(input: unknown): RowWrite[] {
  if (!Array.isArray(input) || input.length < 1 || input.length > 256)
    return invalid();
  return input.map((input) => {
    if (input === null || typeof input !== "object") return invalid();
    const op = (input as Record<string, unknown>).op;
    if (op === "insert") {
      const object = record(input, ["op", "row"]);
      return { op, row: row(object.row) };
    }
    if (op === "update") {
      const object = record(input, ["op", "key", "row"]);
      return { op, key: key(object.key), row: row(object.row) };
    }
    if (op === "delete") {
      const object = record(input, ["op", "key"]);
      return { op, key: key(object.key) };
    }
    return invalid();
  });
}
export function payload(input: unknown): string {
  const bytes = JSON.stringify(input);
  if (encoder.encode(bytes).length > 65536) return invalid();
  return bytes;
}
export function found(input: unknown): RowValue[] | null {
  const object = record(input, ["row"]);
  return object.row === null ? null : row(object.row);
}
export function page(input: unknown, maximum = 128): RowPage {
  const object = record(input, ["rows", "next"]);
  if (!Array.isArray(object.rows) || object.rows.length > maximum)
    return invalid();
  const rows = object.rows.map(row);
  if (rows.some((r) => r.length !== rows[0]?.length)) return invalid();
  const next = object.next === null ? null : key(object.next);
  if (next !== null && rows.length === 0) return invalid();
  return { rows, next };
}
export function changed(input: unknown): RowChanged {
  const object = record(input, ["key", "transaction"]);
  return {
    key: key(object.key),
    transaction: decimal(object.transaction, 1n, (1n << 64n) - 1n),
  };
}
export function batch(input: unknown): BatchChanged {
  const object = record(input, ["changed", "transaction"]);
  const changed = decode.unsigned(object.changed, 256);
  if (changed < 1) return invalid();
  return {
    changed,
    transaction: decimal(object.transaction, 1n, (1n << 64n) - 1n),
  };
}
