# ADR0121: owned native object archive publication

Status: accepted for experimental synthetic-data use.

## Context

The original codec verifies canonical complete archive bytes, but a returned Vec
does not prove a durable selected file. Reopening a final pathname after owned
publication can observe a replacement inode, even with identical bytes. Native
object backups also must not add an unmanaged archive inside their source.

## Decision

Retain the actual destination parent before capture, reject its identity if it
matches the source, capture/encode/recheck the complete bounded source, and reuse
the original storage no-replace publisher. Extend that publisher with an explicit
retained-file result. Clone before selection and preserve all original fsync,
readback, ownership, cleanup and uncertain-result semantics.

Verify the selected archive through that same file descriptor and retained parent;
require the final name inode and full report to match. Recheck visible parent and
the retained source marker. Postselection uncertainty preserves the artifact.
Expose only an offline metadata-output backup CLI with explicit readonly recovery.
Keep restore as a separate implementation and acceptance block.

## Verification and limits

The identical-content final-name substitution test fails before retaining the
original file and passes after the change. Final evidence will record both Rust
toolchains, concurrent complete publication, maximum combined count/bytes,
generated binary histories, parent/source substitutions, original sync faults,
real process kills and actual CLI stdout failure.

The native operator/ancestors remain trusted. Checksums are neither capabilities
nor encryption. Current source verification is observational under the cooperating
owner, not a hostile-filesystem transaction. Multiple complete images can coexist
in memory. Process termination is not a power-loss test. AccountRoot formats do
not change and still exclude objects; user-file HTTP, restore and production
gates remain open.


Executed111 checks on each stable1.99/minimum1.89.0:59 object,35 storage and17
actual CLI. Nineteen new regular cases include24 generated publication histories,
eight competing complete sources, maximum128-object/64 MiB publication and four
new process kills: preselection, selected/unreturned, empty/nonempty received ACK.
All eight old kills rerun. Four new retained-publisher sync faults preserve the
original outcome distinction; actual CLI stdout failure preserves the archive.
Workspace/fuzz format/strict lint, stable CLI/server build, minimum workspace build
and all-fuzz compile pass on555 frozen hashes.
ASAN unchanged archive parser: 2,738,927 inputs in46s, RSS406MiB under512,
max262144/prefix16,940 seeds, no findings. This does not fuzz filesystem publication.
See [verification](../measurements/2026-10-09-object-backup/verification.json).
