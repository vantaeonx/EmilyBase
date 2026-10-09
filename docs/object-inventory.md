# Bounded native object inventory and capture

The native [project directory owner](object-directories.md) can now verify a
complete inventory and copy its exact bytes into a bounded immutable snapshot.
Neither result is a persisted backup, an authenticated manifest or user access
authority. Object directories remain outside current AccountRoot backups.

## Complete admission

Inventory allows at most 128 objects and 64 MiB of combined payload. Each object
still has its independent 8 MiB envelope bound. These are inventory/capture work
limits, not quotas enforced on put: a larger directory can exist and its complete
inventory must refuse. No truncated success or partial list is returned.

Enumerate through the retained directory descriptor, using an independently
opened directory stream so repeated calls do not share an exhausted offset.
Bound encountered entries to 128 plus the marker and dot entries. Only the exact
marker and canonical 39-byte object filenames are accepted. Unknown names,
non-UTF-8/uppercase names, subdirectories, aliases and leftover staging entries
refuse. No unknown file is ignored or deleted. Cleanup remains an operator task.

Sort typed IDs by their 16 raw bytes. Fully read/verify every expected scoped
image; keep at most one payload buffer during inventory while retaining checked
file descriptors. Reject excessive total bytes. Rescan the complete name set,
check original inode/change metadata and reread/hash every retained object again.
Check visible identity after the second read and current directory/marker scope.
Retained descriptors prevent inode reuse from disguising replacement during the
operation. The result contains only project, ordered object IDs, sizes, hashes,
total bytes and the deterministic inventory digest.

This is cooperating-owner consistency plus checked filesystem observations, not
an atomic snapshot against hostile administrators. Same-UID writers/selected
ancestors remain trusted. A writer can act after a final check. Checksums and
metadata do not authenticate a hostile image; advisory locks are not permissions.

## Digest framing

SHA-256 covers the following concatenation, with little-endian integers:

1. Exact bytes `EmilyBase object inventory v1` followed by one NUL byte.
2. Raw 16-byte project ID, u32 object count, u64 total payload bytes.
3. For each sorted object: raw 16-byte ID, u64 payload length, 32-byte payload hash.

Timestamps/inodes do not enter the digest; equivalent independently checked
directories for the same project/content produce the same value. Different
projects, IDs, lengths or payload hashes change the framed input. This is metadata
comparison, not a secret/MAC, persistent format stability promise or capability.

## Immutable capture

`capture` obtains a complete inventory, reads matching exact object bytes into
private owned buffers, then verifies the complete current inventory again.
`capture_inventory` additionally requires a previously observed inventory to
match current project/content first. A stale or foreign receipt refuses; an
equivalent independently verified same-project receipt can match. The receipt
grants no access beyond the already held native directory owner.

The snapshot exposes immutable ordered objects and their inventory. It retains
at most 64 MiB payload plus 128 envelopes/metadata; verification also needs one
bounded temporary payload buffer. This is a logical bound, not measured RSS or a
global heap reservation: callers can retain multiple snapshots. A successful
capture performs five complete payload read passes in exchange for checking
current source before and after copying. Later filesystem changes cannot mutate
the returned byte buffers. Debug hides payloads.

The future archive encoder must publish these verified bytes separately and test
restore; this feature does not create an archive or promise backup durability.

## Offline output and verification

`emilybase object-directory PATH PROJECT list` prints one bounded complete JSON
metadata object with format, project, ordered objects, total bytes and digest.
It never prints payload, silently drops unknown entries or repairs files. Output
failure leaves sources unchanged. The existing output cap remains 65536 bytes.

Tests cover independent digest bytes, insertion order, all 9984 single-byte name
substitutions, all prefixes, full count/byte boundaries, generated metadata/capture
models, source substitutions between phases, stale/foreign receipts and retained
copies after source changes. Native maximum capture is tested separately from the
new filename fuzz target. Fuzz exercises canonical name parsing only, not directory
iteration, filesystem races, snapshot consistency or HTTP/user authorization.
See [ADR0119](adr/0119-bounded-object-inventory-capture.md).

Persistent object archive/restore, root backup integration, inventory persistence,
deletion/staging cleanup, put quotas, HTTP/file policies, signed URLs and production
acceptance remain open. Use synthetic data only.
