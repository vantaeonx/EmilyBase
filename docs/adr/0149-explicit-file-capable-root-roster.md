# ADR0149: define a separate explicit file-capable root roster

Status: accepted for experimental pure codec/inspection only. Live adoption remains
proposed; current AccountRoot, account bundles and ordinary restore remain v1.

## Decision

Add a distinct v2 canonical JSON envelope with version, sorted private-project
roster, sorted file-store bindings and reset time, protected by payload CRC32.
Bind each file project to its exact native file metadata database identity; require
canonical IDs, private membership and unique file database IDs. Cap each roster at
128 and complete bytes at32768 with bounded JSON visitors. Expose checked copied
metadata constructors, exact canonical encoding and a count-only stdin inspector.

Keep old open/restore/capture/manifest readers unchanged. They must refuse v2;
the new codec must refuse v1 rather than synthesize absent file members. Do not
dispatch a new runtime layout before original owner/roster/backup/restore gates
are implemented. The supported behavior in this increment is pure inspection.

Future layout uses fixed files/project/metadata and files/project/objects under an
explicitly selected root. Declared bindings must be compared against actual retained
native owners and registry/private membership, including global database uniqueness.
An intact CRC or copied declaration is never current user/service/file authority.

## Consequences and open adoption

Explicit metadata format choice precedes attaching runtime stores. Existing formats,
session/clock semantics and startup behavior do not change. Unsupported new metadata
refuses without provisioning, migration, repair or root mutation. Empty/subset rosters
are explicit. Offline count bounds do not authorize active owner counts or reserve
combined replay/capture memory.

The later runtime must establish retained ownership and lock order, runtime admission,
combined archive format/compatibility/bounds, coordinated capture/restore and exact
session reset/orphan accounting before user file endpoints. Ordinary startup may not
adopt or create missing stores. Explicit offline adoption must preserve a rollback
source and cannot silently reinterpret old bundles or session reset timestamps.

Golden bytes, generated models, repaired negatives, bounded arbitrary inputs, actual
v1/v2 refusal without source mutation, CLI behavior and a dedicated sanitizer target
verify codec scope. The root integration proposal remains open; no production,
upgrade or complete file-capable backend claim follows from this metadata codec.
