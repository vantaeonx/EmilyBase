# Bounded seekable object archive inspection

`verify_archive_reader(&mut reader, exact_bytes, project)` checks a complete original
EMILYOBK v1 image from a synchronous Read+Seek source and returns ArchiveReport only.
It starts at offset zero regardless of the incoming position. The trusted caller
supplies exact total length and expected project. Invalid total bounds refuse before
any read or seek. Reader implementations must honor their standard contracts.

## Two passes and complete admission

The original exact header decoder is shared with the borrowed byte verifier. It
preserves magic/version, CRC, flags/reserved, scope, count, aggregate and exact body
length admission. First the reader hashes the complete declared body with8192-byte
scratch and probes exact EOF. Only after outer integrity succeeds does it seek to
offset128 and decode all frames. This preserves outer-checksum priority over nested
errors rather than reporting whichever malformed inner field happens to arrive first.

Each ordered frame has a bounded typed ID/length. Its nested original object is
fully verified through a length-limited reader: the nested EOF probe cannot consume
the next frame. All scopes, versions, CRCs, payload hashes, strict unique ordering,
exact frame/body ends and aggregate payload bytes must pass. At most128 small
metadata tuples are reserved after bounded header admission; the original canonical
inventory digest is recomputed completely. No body/payload image is materialized.
Successful data reads total2N-128 bytes for an N-byte image, with one extra one-byte
EOF probe; bad trailing input consumes at most the declared total plus that probe
before refusal. Interruptions do not establish a time/deadline bound.

This is an additional seekable metadata API, not a single-pass network parser,
authorization capability, retained snapshot lease or whole-process memory quota.
It trades another file pass for avoiding a complete archive copy. It must not be
used to claim upload admission or guaranteed throughput.

## Native path

Native archive inspection and existing selected-archive readback use this reader
under the actual retained private file. Regular/single-link0600/0400 and size checks
precede reads; metadata stability and visible inode checks still surround admission.
Controlled between-pass mutations require refusal without repair or cleanup.
The offline object-archive-verify command still emits metadata only after complete
success. Existing capture, archive encoding and restore keep their owned images and
durability rules. No stored bytes, version, fsync point or format upgrade changes.

The [original byte format](object-archive-format.md) remains authoritative. See
[ADR0129](adr/0129-bounded-seekable-object-archive-inspection.md) for the decision
and source-bound evidence for executed checks and local memory observations.

## Executed evidence

Stable1.99/minimum1.89 each passed181 relevant checks:111 object,44 storage and26
CLI, on575 frozen hashes. Nine new regular cases include64 generated binary
archives, six native between-pass mutations, read/seek/EOF faults, chunk/interrupt/
byte bounds and exact maximum128-object/64 MiB CLI admission with late corruption.
Independent Python bytes, all prefixes/byte damage, repaired fields, nested scope,
ordering and complete inventory digest are compared with the original byte path.
Existing20 kills reran; no new publication boundary or kill is claimed.

Formatting, strict stable workspace/fuzz lint, builds and minimum all-fuzz compilation
pass. The expanded ASAN byte/reader target completed756330 inputs/46s/max262144,
project prefix16, RSS391 MiB under512, no findings. It does not fuzz filesystem
operations or prove hardware power-loss recovery.

Five fresh actual CLI processes per source/shape, with identical existing files and
reports, measured peak RSS using GNU time. For an eight-object64 MiB payload archive,
median RSS was72416 KiB on8c8f097 versus8028 KiB here. For one64 KiB object,
7664 versus8044 KiB shows no small-shape improvement. Development-profile loader/
process variation and source page-cache costs remain distinct; this is not a speed
or global memory quota claim. See
[source-bound report](measurements/2026-10-10-archive-reader/verification.json).
