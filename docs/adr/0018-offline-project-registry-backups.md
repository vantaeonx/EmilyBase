# ADR 0018: bounded offline project-registry backups

Status: accepted for the experimental Linux/local-filesystem implementation.

## Context

Single-database backups preserve committed history but omit project identities,
display names and rotated access digests. Copying live files independently can
mix states, retain abandoned writes or lose registry metadata. Restoring caches
without their mandatory journals must remain forbidden.

## Decision

Add a separate version-1 EMILYREG envelope containing canonical project metadata
and existing independently verified EMILYBAK archives, sorted by project ID.
Limit the whole image to 128 MiB and 128 projects. Keep single-database formats
unchanged. Validate bounds, canonical metadata, unique project/database identities,
checksums and every nested committed history before accepting an image.

Capture requires exclusive registry ownership and no outstanding authorized
capabilities. Acquire all database directory/WAL owners before reading any
committed prefix; keep them until capture and source revalidation finish. Direct
CLI writers are refused while those owners are held. Preserve only committed
history, current key digest/epoch, database identities and transaction IDs.

Publish a read-back-verified private file with no-replace rename and parent sync.
Restore privately into a new registry: replay all WALs, regenerate caches, compare
the recaptured complete image with the archive, sync directories, no-replace rename
and sync the parent before success. Any post-rename parent open/sync failure is
an unknown publication outcome, requiring inspection rather than automatic retry.

## Consequences and open gates

Backups are offline; stop the server. Restores retain project credentials and
rotation epochs; master credentials stay external. The image contains plaintext
data and key digests, so files are private and excluded from Git. Checksums detect
accidental corruption and do not authenticate a maliciously rewritten archive.

The implementation retains bounded images and recovered snapshots in memory;
it is not a streaming/low-memory archive. Operators need memory for these buffers
and replay. Source staging, unrelated standalone indexes, object files, sessions
and unimplemented platform components are outside this format. Existing paths are
never overwritten. Live snapshots, encryption, cross-version conversion, physical
power-loss, broader security/load and production gates remain open. Dedicated
process-kill/sync-failure, independent model, capacity and restored HTTP/container
campaigns now execute; broader failing-media acceptance is still separate work.
