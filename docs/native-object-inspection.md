# Native directory metadata inspection

`ProjectDirectory::inspect(ObjectId)` fully verifies one selected object and returns
its exact payload length and SHA-256. It uses the original streamed object decoder:
96-byte header,8192-byte payload scratch, complete payload hash and exact EOF.
It never constructs or returns an owned payload image. Scope checks and report/file
metadata still have their own small costs; this is not zero-allocation or total
process memory admission.

The retained private directory and current marker are checked before lookup. The
typed identifier selects a canonical name with descriptor-relative no-follow open.
The actual verified file stays open across the final project-marker check; private
metadata/stability and visible inode are then checked again. Detected changes fail
without a report or repair. Same-byte replacements, links, mode changes, truncation
and scope substitution are not adopted. The operation is a checked snapshot, not a
filesystem lease or protection against an administrator controlling the filesystem.

Renaming the owned directory preserves its original descriptor namespace; this
does not switch to a new directory installed at the former path. Inspecting one
object does not certify unrelated entries or complete inventory. Use `inventory`
for the latter. Existing owned `get`, capture, backup/restore and write APIs retain
their contracts and full images where required.

The unchanged offline command now uses this report-only path:

```sh
emilybase object-directory ./synthetic-objects 01010101010101010101010101010101 inspect 02020202020202020202020202020202
```

It emits metadata JSON only after complete verification, including for a private
readonly8 MiB object; late corruption produces no partial stdout. No HTTP file
service, user authorization, persisted quota, format/version or durability change
is introduced. See [ADR0128](adr/0128-native-streamed-object-inspection.md).

## Executed checks and local observations

Stable1.99/minimum1.89 each passed172 relevant checks:103 object,44 storage and25
actual CLI. Seven new regular cases include64 generated native payloads and nine
post-verification mutation shapes. Existing20 process-kill scenarios reran; no new
kill/publication boundary is claimed. Formatting, strict workspace/fuzz lint,
builds and minimum fuzz compilation pass on572 frozen source hashes.

Five fresh actual CLI processes per source/shape, using the same synthetic files
and GNU time's peak RSS, preserved every metadata report. For8 MiB, median RSS was
15796 KiB on d988a51 versus7916 KiB on this increment. For64 KiB,7604 versus7832
KiB shows no small-object improvement; loader/process variation remains. This is
one local development-profile observation, not a heap reservation or throughput
claim. The unchanged byte/reader ASAN target also ran937259 inputs/46s/RSS468 MiB
under512 without findings; it does not call the new native filesystem API.
See [source-bound evidence](measurements/2026-10-10-native-object-inspection/verification.json).
