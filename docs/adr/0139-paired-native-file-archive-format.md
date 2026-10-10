# ADR0139: checked paired native file archive format

Status: accepted experimental byte encoding; durable publication/restore pending.

## Decision

Introduce a standalone native archive with one fixed192-byte header followed by
exactly one existing original-engine metadata backup and one existing complete
object archive. Header binds project, database identity and last acknowledged
transaction; it gives bounded exact component lengths, two SHA-256 digests, reserved
zero bytes and header CRC. Versions and canonical nested formats are verified.

Both outer digests precede nested replay. Shared private-schema validation binds
metadata scope and revision. Every reference must resolve to the correct physical
object hash/length and persisted quota must fit the whole archive, including orphans.
No gaps, duplicates, overlap, noncanonical padding or trailing bytes are admitted.

Expose a borrowed Read/Seek encoder from immutable FileSnapshot or fully verified
archive. It hashes using8 KiB scratch and retains borrowed component images without
allocating a second complete encoded payload. Pure byte verification retains a
bounded replayed metadata snapshot transiently and borrows object payloads.

## Consequences

Checksums detect corruption, not malicious origin. Expected project is required;
matching identities do not prove unique ancestry, current authority or a native
target selection. Existing component versions are unchanged. This combined format
is experimental and introduces no claim of production compatibility.

Native immutable images still cost bounded memory; this is not server heap/FD
admission. Wire decoding grants no path, account or user permission. A future
common owned no-replace publication and common restore must verify/read back the
whole pair before selecting one destination and must pass their own crash matrix.
