# Borrowed canonical object archive encoding

`ArchiveReader::from_snapshot(&snapshot)` and `from_verified(&archive)` expose the
original EMILYOBK v1 bytes through synchronous Read+Seek. Both inputs already own
or borrow completely verified immutable object images. Arbitrary raw input cannot
construct this reader. The lifetime retains the source borrow; moving or removing
the original native directory cannot change a captured snapshot's bytes.

Construction checks aggregate bounds, strict ID order, each image length and exact
body length. It hashes canonical frame/image bytes once and builds the original
128-byte header, sharing its encoder with the existing owned byte encoder. It
retains at most128 descriptors, each holding a24-byte frame, output offset and
borrowed image slice. Payloads are neither copied nor retained a second time.
Read/Seek performs no payload or output allocation; output belongs to the caller.
The complete owned snapshot still consumes its original bounded memory. This is
not a whole-process memory reservation or a streaming capture of live files.

Reads cross header/frame/object boundaries and return exact EOF without padding.
Seeking past EOF is allowed; negative or overflowing relative positions refuse
without changing the position. Empty reads leave position unchanged. The reader
borrows immutable bytes rather than trusting a second read of a mutable source.
Debug exposes counts, encoded length and position, never payload data.

The original owned encode_archive/encode_verified_archive APIs remain available
and preserve their bytes. The [format contract](object-archive-format.md), native
capture admission, versions, fsync points and publication uncertainty are unchanged.
At this increment native backup still builds its owned encoded image. Connecting
the reader to durable publication requires a separately tested publisher with
complete stage readback, retained selected inode and original no-replace/fsync
outcomes. This library API adds no HTTP route, file authority or release acceptance.

See [ADR0130](adr/0130-borrowed-canonical-object-archive-reader.md) and the
[seekable metadata verifier](seekable-object-archive-inspection.md).

## Executed checks

Stable1.99 and minimum1.89 each passed188 relevant checks:118 object,44 storage
and26 CLI. Seven new regular cases include64 generated binary archives and seek/
read sequences, independent known bytes, actual removed native source, borrowed
slice identity, segment boundaries, checked arithmetic, empty/max128-object/64 MiB
shapes and private constructor bounds. Existing20 process kills reran; no new kill
or publication boundary is claimed. All577 frozen source/config hashes match.

Formatting, strict stable workspace/fuzz lint, builds and minimum all-fuzz checks
exit0. The expanded ASAN target completed761600 inputs/46s/max262144/prefix16,
RSS392 MiB under512, without findings. An earlier missing-corpus setup attempt
executed no inputs and is excluded. This target checks actual encoded-reader bytes
and boundary positions in addition to byte/reader admission and owned re-encoding.
It does not exercise native filesystem faults. See
[source-bound evidence](measurements/2026-10-10-archive-encoding/verification.json).
The earlier complete1518-test d988a51 baseline is not a full result for this source.
