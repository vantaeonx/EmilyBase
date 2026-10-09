import { EmilyBaseError } from "./types.js";

export async function readBody(
  response: Response,
  maximum: number,
): Promise<unknown> {
  const length = response.headers.get("content-length");
  if (length !== null && /^\d+$/.test(length) && Number(length) > maximum) {
    await response.body?.cancel().catch(() => undefined);
    throw new EmilyBaseError("response_limit", "unknown", response.status);
  }
  if (response.body === null)
    throw new EmilyBaseError("protocol_error", "unknown", response.status);
  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let size = 0;
  let text = "";
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > maximum)
        throw new EmilyBaseError("response_limit", "unknown", response.status);
      text += decoder.decode(value, { stream: true });
    }
    text += decoder.decode();
    try {
      return JSON.parse(text) as unknown;
    } catch {
      throw new EmilyBaseError("protocol_error", "unknown", response.status);
    }
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}
