# ADR 0060: admit each projected row before cloning its selected payload

Status: accepted for the original SQL executor. Formats and durable ACK unchanged.

## Reproduced problem

The common final projection cloned all selected rows, then checked the script
output budget. A valid primary JOIN with 1500 rows per side, 3072 hidden bytes per
source row, 32 selected copies and LIMIT 1000 returned the expected output-limit
error, but a native warmed release regression first observed a 99388593-byte
requested peak. This exceeds the intended 8-MiB logical output allowance before
refusal. The actual native guard failed against published 80b51ef before repair.
Single-row streamed paths also cloned their next row before budget admission.

## Decision

Use one synchronous private projection helper across full sorting, bounded JOIN
selection and streamed single-table/JOIN output. On a borrowed full candidate,
validate every resolved selected index and calculate the original logical row
charge: 24 plus 32 per selected cell plus each selected text/bytes payload length.
Count every repeated selected occurrence. Use checked addition, then charge the
shared script output budget before cloning any selected value or result vector.
The already bound immutable candidate cannot change between preflight and copying.
A malformed internal index returns a typed plan error; no unchecked input access
occurs before validation. No new public ownership API or async layer is introduced.

The common projection processes its limited sorted rows one by one. It never
constructs all projected results before checking their shared allowance. Failure
drops retained input, previous local output and the staged transaction through
existing ownership. Headers, aliases, duplicates, NULL, finite float bits, row
order, output-limit error and exact committed bytes retain their prior meanings.
The existing logical charge formula and 8-MiB script limit do not change. Binding
still precedes LIMIT0/empty input, and the explicit 64-column parser limit remains.

This admits logical selected payload, not allocator usable-size, header/metadata,
cold source/cache construction, staged state, temporary predicates or whole-process
memory. Full-sort inputs and bounded candidate heaps retain their separate bounds.
All previous work/result-row/transaction-event limits and durable rules remain.

## Evidence and limits

The same sorted primary-JOIN sample now observes requested peak 14210993 bytes,
with the same output-limit error and zero retained allocation. The earlier peak
was 99388593. Final operation-local samples for single-table sort, primary-order
stream and fallback JOIN are 12889830/8355020/14189897, all refused with zero retained
allocation. Fixtures/caches precede profiling. Exact source hashes and excluded
allocator overhead/rounding, stacks and profiler data appear in
[source-bound observations](../measurements/2026-10-07-projection-admission/operation-peaks.json).
None of these observations is a process/transient quota or cold-memory estimate.

Private checks inspect actual shared charges, repeated/empty/null payloads, finite
float bits, unchanged work and invalid-index refusal. Public checks verify exact
last-admitted/first-refused byte boundaries across all read plans, aliases/star/
types, the existing 64-column parser cap, complete empty/LIMIT0 binding and a 64-case
independent nullable projection model. Both managed WAL versions preserve exact
committed journal/state after a staged write followed by cumulative output refusal,
rollback, checkpoint/reopen and verified backup/restore with independent writes.
Parser/format code, SQL grammar, API fields, dependencies and unsafe policy are unchanged.

The combined durable table/index writer, numeric model/cache/staging/transient
admission, broader fault campaigns and production acceptance remain open.
