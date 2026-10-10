# ADR0130: borrowed canonical object archive reader

Status: accepted for experimental synthetic-data use.

## Context

Native backup captures verified immutable object images, then duplicates their
payloads into a complete encoded archive. A reader over live files would weaken
the immutable capture boundary and need a different concurrency contract. A
borrowed reader over the existing capture can avoid the second image while
retaining the original exact format and independent owned encoder.

## Decision

Add a synchronous Read+Seek view constructed only from ObjectSnapshot or
VerifiedArchive. Bound count/bytes before descriptor reservation. Retain small
ordered frames/borrowed slices, hash the body once and share the original header
encoder. Reads/seeks copy only into caller output and follow standard cursor
semantics, including past EOF and checked relative arithmetic. Keep Debug private.

Keep the owned encoder and native publisher unchanged in this increment. Durable
publication integration must separately prove bounded exact input, complete byte
readback, owned staging/selection and uncertainty after directory synchronization.

## Consequences and acceptance

The source capture remains owned and bounded. Descriptor allocation and hashing
are real work; no global memory quota, timing bound or throughput claim follows.
There is no new stored version, public authority or project/user endpoint.

Require independent known bytes, both verified-input constructors, retained source
slice identity, empty/maximal shapes, all segment boundaries, EOF/empty reads,
negative/overflow seeks and generated seek/read comparison against the owned byte
cursor. Extend the existing archive sanitizer comparison to actual encoded-reader
bytes and positions. Run relevant native/CLI checks on stable and minimum Rust;
record frozen source hashes and do not reuse an earlier broad workspace count as
evidence for this source. See [contract](../borrowed-object-archive-encoding.md).

Executed188 relevant checks per stable/minimum toolchain on577 frozen hashes;
seven new regular cases include64 generated archive/seek sequences. Expanded ASAN
comparison:761600 inputs/46s/RSS392 under512, no findings. The contract records
the excluded missing-corpus setup attempt and precise scope. Native durable backup
integration remains separate; no production or full-workspace gate closes here.
