import { InvalidProtocol } from "./decode.js";
import type { UserSession, UserInfo } from "./types.js";

const encoder = new TextEncoder();
function invalid(): never {
  throw new InvalidProtocol();
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
export function login(input: unknown): string {
  if (
    typeof input !== "string" ||
    input.length > 64 ||
    !/^[a-z0-9][a-z0-9._-]*$/.test(input)
  )
    return invalid();
  return input;
}
export function password(input: unknown): string {
  if (typeof input !== "string" || input.length < 1 || input.length > 1024)
    return invalid();
  for (const point of input) {
    const unit = point.charCodeAt(0);
    if (point.length === 1 && unit >= 0xd800 && unit <= 0xdfff)
      return invalid();
  }
  if (encoder.encode(input).length > 1024) return invalid();
  return input;
}
export function token(input: unknown, kind: "access" | "refresh"): string {
  if (typeof input !== "string" || input.length !== 102) return invalid();
  const prefix = kind === "access" ? "eba1_" : "ebr1_";
  if (
    !input.startsWith(prefix) ||
    !/^[0-9a-f]{32}\.[0-9a-f]{64}$/.test(input.slice(5))
  )
    return invalid();
  return input;
}
function decimal(input: unknown, maximum: bigint): string {
  if (
    typeof input !== "string" ||
    input.length > 20 ||
    !/^[1-9][0-9]*$/.test(input) ||
    BigInt(input) > maximum
  )
    return invalid();
  return input;
}
export function session(input: unknown): UserSession {
  const object = record(input, [
    "access_token",
    "refresh_token",
    "token_type",
    "expires_at",
  ]);
  const access_token = token(object.access_token, "access");
  const refresh_token = token(object.refresh_token, "refresh");
  if (
    object.token_type !== "Bearer" ||
    access_token.slice(5, 37) !== refresh_token.slice(5, 37)
  )
    return invalid();
  return {
    access_token,
    refresh_token,
    token_type: "Bearer",
    expires_at: decimal(object.expires_at, (1n << 63n) - 1n),
  };
}
export function user(input: unknown): UserInfo {
  const object = record(input, ["id", "login", "credential_epoch", "disabled"]);
  if (
    typeof object.id !== "string" ||
    !/^[0-9a-f]{32}$/.test(object.id) ||
    typeof object.disabled !== "boolean"
  )
    return invalid();
  return {
    id: object.id,
    login: login(object.login),
    credential_epoch: decimal(object.credential_epoch, (1n << 64n) - 1n),
    disabled: object.disabled,
  };
}
export function logout(input: unknown): { logged_out: true } {
  if (record(input, ["logged_out"]).logged_out !== true) return invalid();
  return { logged_out: true };
}
export function payload(input: unknown, maximum: number): string {
  const body = JSON.stringify(input);
  if (encoder.encode(body).length > maximum) return invalid();
  return body;
}
