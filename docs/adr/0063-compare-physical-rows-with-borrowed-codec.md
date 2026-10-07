# ADR 0063: compare physical live rows through the shared borrowed cell codec

Status: accepted for original snapshot row-location resolution. Formats unchanged.

## Reproduced problem

A checked physical resolution decoded an entire ETBL row to an owned vector and
then compared it with the already validated immutable model row. Text primary
keys were also copied while checking the decoded schema key. Against published
5b416eb, a warmed release test resolving 1000 rows requests 3344000 allocation
bytes for integer keys/wide hidden text and 6416000 for 3072-byte text keys. Its
native guard fails before implementation, including the final 128-KiB bound.

## Decision

Use one bounded validating cell reader for original EROW owned decode and a new
row_matches comparison. Scalars are stack values; text/bytes borrow input slices.
Owned decode copies validated cells only when a caller actually needs ownership.
The matcher reads every declared cell and checks the exact record end even after
an early field/count mismatch. It validates magic/version, count/record bounds,
Boolean encodings, tags, finite floats, UTF-8, value sizes and trailing data.
Mismatches return false; malformed encodings retain typed catalog errors. Numerical
float equality preserves original Value equality, including equal signed zeros;
owned decode still preserves their stored bits. No unsafe or lifetime escape.

ETBL envelope parsing is shared with original Event decode. Live row comparison
accepts only fully validated Insert/Replace images of the expected table. Non-row
kinds retain their complete original kind-specific decode before returning false.
Row-location resolution still verifies current table/key/location identity,
selected page/slot, exact record digest, complete physical payload and the current
primary value. Immutable model rows were schema validated on creation/replay.
The current primary value is compared with the borrowed requested key directly,
without constructing another owned key. Valid physical/model divergence returns
StaleLocation. Hash/format checks are not replaced by trusting the cached row.

No EROW/ETBL/EBPG/EMILYDB/WAL/cache bytes, versions, SQL/API contract, commit ACK or
async storage change. The original owned codec remains available. Schema/key
validation remains; this is payload-copy removal rather than a process quota.

## Evidence and remaining costs

The same integer/text samples each request total104000 bytes in1000 blocks,
peak104 and retain zero. Per-call schema validation still allocates. Exact hashes,
dimensions and exclusions appear in [observations](../measurements/2026-10-07-physical-row-match/operation-allocations.json).
An initial16-KiB guard also exposed that metadata cost; it was adjusted to128 KiB
and rerun against both published baseline and repaired source. These requested
operation-local totals exclude cold fixtures, allocator overhead/rounding, stacks
and profiler data; they describe neither throughput nor RSS.

Explicit tests preserve all value kinds, empty/count-mismatched rows, exact value
bounds, signed-zero behavior, truncated prefixes, malformed UTF-8/Boolean/float/
tag/version/oversized values, trailing errors after early mismatch and non-row
ETBL validation. Independent generated values and arbitrary-byte models execute.
Existing row-location, stale-pointer, all-type replay, old-view, SQL, WAL and
backup recovery checks remain required. The new matcher has a bounded ASan target.

Numeric cache/model/staging/transient budgets, combined durable writer and broader
power-loss/security/production gates remain open.
