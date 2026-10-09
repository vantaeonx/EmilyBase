# ADR0126: stream complete object metadata without owning payload images

Status: accepted for experimental synthetic-data use.

## Context

Native inspection and both complete inventory passes allocated/read an entire
object image only to discard its payload and keep a length/hash report. Objects
are bounded at8 MiB, but that transient copy is unnecessary for metadata. Splitting
a reader implementation must preserve complete verification and avoid duplicate
header rules, premature reports or weakened visible-inode checks.

## Decision

Factor exact v1 header admission into one crate-private decoder shared by the
original whole-byte verifier and an additive bounded reader verifier. The latter
uses an8192-byte payload scratch, hashes all bytes, requires exact length/EOF and
returns metadata only. Native inspection/inventory retain file ownership, private
metadata checks, before/after stability and visible identity while streaming reports.
Keep owned get/capture/publication payload images and archive behavior unchanged.

## Consequences and acceptance

Encoded bytes, version support and canonical scopes remain stable. Reader byte
bounds do not impose deadlines, authorize user access or reserve total process
memory. Native regular-file/worker constraints remain mandatory. Existing complete
inventory bounds and cooperating-owner semantics continue to apply.

Require independent vectors, chunk/error/boundary/prefix/repaired-header tests,
generated binary readers, actual native mutation tests, both supported Rust
toolchains and an executed sanitizer comparison before publishing this increment.
No file HTTP, global memory quota, AccountRoot or production gate closes here.
See [contract](../object-stream-verification.md).


Executed165 checks per stable1.99/minimum1.89.0 on570 frozen hashes. Nine new
regular cases include64 generated readers and native post-hash mutation refusal.
Existing20 kills/models rerun. Final ASAN comparison:666631 inputs/46s/RSS437
under512, max262144, prefix32, no findings. Formatting/strict lint/build/minimum
fuzz compilation pass. See [verification](../measurements/2026-10-10-object-stream/verification.json).
