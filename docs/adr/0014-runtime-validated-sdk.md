# ADR 0014: runtime-validated project SDK and uncertain write outcomes

Status: accepted for the first TypeScript SDK.

## Decision

Use a small Fetch-based ESM client without runtime dependencies. Expose only
existing project-scoped SQL, explain and status endpoints. Separate administrator
operations, never persist/serialize credentials and disable redirect following.
TypeScript types are accompanied by runtime decoding of untrusted responses and
input byte/shape/value bounds; erased compile-time types alone are insufficient.

Support safe JavaScript integers explicitly. The current JSON i64/u64 protocol
has no exact BigInt representation; larger integer results fail with unknown write
outcome rather than silently rounding. A future extended integer wire contract
needs an API compatibility decision. SQL bindings remain separate values.

Classify fixed refusal codes only with their expected status. Never copy arbitrary
server error text. Every disconnect/cancellation/invalid success may have an
unknown write outcome. No implicit retries or request-id deduplication. Abort can
stop response observation while the synchronous engine still commits.

## Consequences

This client has full access within its configured project key scope; there are
no user/row policies yet. Browser use requires same-origin proxying until server
CORS is designed. Node 22 live tests prove real HTTP/restart behavior; browser
and mobile checks remain pending. The package is not published to npm. There is
no realtime, object or user/session API falsely exposed as a working feature.
