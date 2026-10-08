import * as decode from "./decode.js";
import type {
  MigrationApplied,
  MigrationDefinition,
  MigrationReceipt,
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
function version(input: unknown): number {
  const value = decode.unsigned(input, 128);
  return value < 1 ? invalid() : value;
}
function label(input: unknown): string {
  if (
    typeof input !== "string" ||
    !/^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$/.test(input)
  )
    return invalid();
  return input;
}
function sql(input: unknown): string {
  if (typeof input !== "string" || input.length < 1 || input.length > 16384)
    return invalid();
  // Replacing an unpaired surrogate would silently change exact migration bytes.
  for (const point of input) {
    const unit = point.charCodeAt(0);
    if (point.length === 1 && unit >= 0xd800 && unit <= 0xdfff)
      return invalid();
  }
  return encoder.encode(input).length > 16384 ? invalid() : input;
}
export function definition(input: unknown): MigrationDefinition {
  const object = record(input, ["version", "label", "sql"]);
  return {
    version: version(object.version),
    label: label(object.label),
    sql: sql(object.sql),
  };
}
export function payload(input: MigrationDefinition): string {
  const body = JSON.stringify(input);
  return encoder.encode(body).length > 65536 ? invalid() : body;
}
function transaction(input: unknown): string {
  if (typeof input !== "string" || !/^[1-9][0-9]{0,19}$/.test(input))
    return invalid();
  const value = BigInt(input);
  return value < 2n || value > (1n << 64n) - 1n ? invalid() : input;
}
function receipt(input: unknown): MigrationReceipt {
  const object = record(input, ["version", "label", "sha256", "transaction"]);
  if (
    typeof object.sha256 !== "string" ||
    !/^[0-9a-f]{64}$/.test(object.sha256)
  )
    return invalid();
  return {
    version: version(object.version),
    label: label(object.label),
    sha256: object.sha256,
    transaction: transaction(object.transaction),
  };
}
export function inventory(input: unknown): MigrationReceipt[] {
  const object = record(input, ["migrations"]);
  if (!Array.isArray(object.migrations) || object.migrations.length > 128)
    return invalid();
  const values = object.migrations.map(receipt);
  let previous = 1n;
  for (let i = 0; i < values.length; i++) {
    const value = values[i]!;
    if (value.version !== i + 1 || BigInt(value.transaction) <= previous)
      return invalid();
    previous = BigInt(value.transaction);
  }
  return values;
}
export function applied(
  input: unknown,
  expected: MigrationDefinition,
): MigrationApplied {
  const object = record(input, ["receipt", "already_applied"]);
  const value = receipt(object.receipt);
  if (
    typeof object.already_applied !== "boolean" ||
    value.version !== expected.version ||
    value.label !== expected.label
  )
    return invalid();
  return { receipt: value, already_applied: object.already_applied };
}
