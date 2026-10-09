# ADR0119: complete bounded object inventory and immutable capture

Status: accepted for experimental synthetic-data use.

## Context

Backup cannot rely on a partial directory listing or unchecked object headers.
Native object operations already retain project/directory ownership but have no
complete inventory, coherent bounded byte input for an archive or resource limits
for traversing unknown filesystem entries.

## Decision

Add complete native inventory bounded to 128 objects/64 MiB payload, preserving
the individual 8 MiB envelope bound. Traverse the retained directory through a
separate descriptor stream; refuse unknown/staging/invalid entries rather than
silently skipping them. Bound entry enumeration and metadata allocation. Fully
verify scoped images, retain checked descriptors, rescan names and reread/hash
every original inode before returning ordered metadata. No partial result escapes.

Fingerprint the exact domain/project/count/total and sorted ID/length/hash framing
with SHA-256. This is a deterministic metadata comparison, not authentication,
persistent manifest stability or user authority. Add a bounded canonical filename
decoder and fuzz its actual implementation separately from the envelope decoder.

Capture immutable original object bytes matching a complete checked inventory,
then reverify the complete source. A previously observed receipt must match current
project/content; it conveys no new filesystem permissions. The snapshot holds
bounded bytes for a future archive encoder but publishes no archive. Five complete
payload passes favor verification over throughput at this experimental stage.

Expose metadata-only offline list. Keep the existing output bound and no-repair
behavior. Do not introduce fake backup/restore or HTTP implementations.

## Verification and consequences

Final evidence will record both toolchains, independent digest vector, every-byte
filename cases, real 128-object/64 MiB boundaries, maximum immutable capture,
generated metadata/capture histories, delayed substitutions, stale receipts,
real CLI/full list/output failure and ASAN of the filename decoder. Existing
object/publication/directory recovery checks rerun; no new readonly process-kill
or durable backup proof is implied.

These are inventory/capture work bounds, not write quotas or a global retained
heap admission. Unknown/stale staging files deliberately prevent complete backup
input until separately inspected. Native locks coordinate cooperating owners;
checked phases are not atomic against a hostile administrator. Scope/hash checks
are not encryption or an access-policy engine. Root roster integration, persistent
archive/restore, deletion/cleanup, HTTP, quotas and production gates stay open.


Executed81 checks each on stable1.99/minimum1.89.0:38 object,33 storage and10
actual CLI checks. Seventeen new regular cases include24 inventory/capture models,
24 filename draws,9984 single-byte substitutions, full128-object/64 MiB boundaries
and maximum immutable capture. Three harness workers are explicitly invoked by
parents; all eight existing object/directory/raw-creation kills rerun. Three new
CLI cases cover complete128-object output, no partial errors and readonly stdout
failure. Workspace/fuzz format/strict lint, stable CLI/server build, minimum
workspace build and all-fuzz compile pass on547 frozen source/config hashes.
ASAN object_name executes43,816,723 inputs in46s, RSS304MiB under512,
max input1024/canonical39 bytes,378 reported seeds, no findings.
This tests filename grammar only, not filesystem consistency or authorization.
See [verification](../measurements/2026-10-09-object-inventory/verification.json).
