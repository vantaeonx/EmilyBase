# ADR 0040: opt-in synthetic allocation diagnostics

Status: accepted for diagnostic tooling only. No runtime admission or writer gate.

## Context

The aggregate image bound and cached canonical hashes in ADR 0039 cover encoded
objects, not Rust heap use. The in-memory model clones relational state at begin,
and immutable readers can retain the prior state after publication. Independent
page maxima cannot measure that cost or authorize four server workers.

## Decision

Add a separate unpublished `model-profile` crate. Its `heap-profile` feature
enables a standalone release binary with the pinned ordinary allocation profiler
`dhat` 0.3.3. The original database/index code remains the storage implementation.
The server and CLI runtime graphs do not depend on this crate or allocator.

Generate bounded synthetic integer, exact-256-byte UTF-8 and exact-3072-byte
UTF-8 keys. State runs hold 1..4 independent project models with 1..10000 rows
each and 0..768-byte second-column values. Construction and publication are
serial; private stages and optional old views are held simultaneously. Verify
new/old values and row counts before release. Long keys remain on the relational
path and have a one-page empty short-key tree. No database files or WAL are written.

Fingerprint mode compares `SHA256(snapshot.encode())` with the canonical streaming
method using the same valid trees. Preallocate the digest collection before the
comparison; verify equality and report checked allocation-traffic differences.
The cumulative peak includes both methods and cannot show separate operation peaks.

Only bounded, versioned, count-only JSON is emitted. Profiler testing mode disables
its automatic file output. Errors omit invalid arguments and supplied JSON. The
8192-byte decoder rejects unknown fields, invalid phase order, inconsistent
components/counters and impossible allocation differences. Codec properties and
the fuzz target work without enabling the diagnostic allocator. A separate CI job
activates the feature and runs actual child processes, plus minimum-Rust checks.

## Consequences

Requested allocation counters expose clone/retention costs and can guide later
admission design. They include diagnostic collections and allocation traffic from
model construction, and exclude the profiler's own bookkeeping. Linux maximum
RSS of the instrumented process is recorded separately. Neither is a worst-case
bound, throughput claim or memory quota. Reports are unsigned observations;
structural validation does not authenticate a submitted measurement.

Retained-view lifetime, simultaneous worker transients, replay/compaction/history
growth, an enforceable reservation scheme and complete WAL-byte accounting remain
open. Existing formats, ACKs, backup/API/SDK shapes and durable selection are
unchanged. See [reproduction and measurements](../model-allocation-profiles.md).
