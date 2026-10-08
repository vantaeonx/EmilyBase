# ADR0099: Exact bounded migration transport in the TypeScript SDK

Status: accepted for experimental trusted backend clients.

Expose migrationList and migrationApply using the existing project-scoped transport,
no runtime dependency and no administrator authority. Snapshot strict definitions
before dispatch. Validate version/ASCII label/UTF8 SQL and complete JSON body bounds;
reject lone UTF16 surrogates before TextEncoder can replace definition bytes.
The Rust parser/engine remains the authority for syntax, schema and atomic writes.

Decode bounded receipt metadata with exact canonical decimal u64 strings (minimum2)
and lowercase64-hex digests. Require consecutive inventory versions and increasing
commit IDs, maximum128 entries. Check applied receipt version/label against its
snapshot. These are structural checks, not client digest recomputation, signatures
or independent audit authority. Bound streamed responses at65536 bytes.

Recognize migration_rejected only with400 as a safe no-commit refusal. Retain
migration_history_invalid and ambiguous commit/response/transport as unknown.
Preserve existing cancellation, no-cache/no-cookie/no-redirect, static redaction,
key replacement and close behavior. Never retry, skip versions or rewrite SQL
implicitly. An explicit identical historical retry returns the original receipt.

Acceptance covers independent metadata sequences, full u64 precision, malformed
fields/order/hash/UTF8/bounds, snapshotting and no-dispatch refusals. Actual TCP
checks compare a separate Node SHA256 oracle, simultaneous exact retries, schema
copy, received-ACK kills, deliberately lost SDK response followed by restart/exact
retry, sibling denial and key replacement on both Rust versions. Hosted container
checks remain distinct. User/RLS authority, online coordination and production
acceptance stay open.
