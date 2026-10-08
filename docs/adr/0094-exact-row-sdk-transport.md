# ADR0094: Exact typed row methods in the TypeScript SDK

Status: accepted for experimental trusted service clients.

Expose rowGet/Page/Insert/Update/Delete/Batch through the existing scoped Fetch
transport. Retain private key fields, fixed routes, no redirects/cookies/referrers,
no caching/retries, explicit cancellation and unknown-write outcomes. Add the
fixed table_rejected400 refusal without treating arbitrary errors as safe.

Keep older SQL numeric Value behavior separate. New RowKey/RowValue use canonical
i64 decimal strings and finite binary64 hex bits; response transaction IDs use
positive u64 decimal strings. BigInt checks range without converting to Number;
no raw BigInt is serialized. Validate and snapshot all row/batch values before
Fetch. Apply65,536-byte request/response caps, page/batch bounds and strict output
shapes/consistent widths. Keep SQL's older64 MiB response cap unchanged. Actual
schema/physical-record validation and transactions remain entirely in Rust.

Test all methods with mocked protocol refusals and actual stable/minimum Rust TCP
servers, including exact extrema, atomic late rollback and ACK-kill/reopen. Do not
publish an npm release or claim browser/user-token/RLS authority. No runtime
dependency, Rust engine change or production milestone is introduced.
