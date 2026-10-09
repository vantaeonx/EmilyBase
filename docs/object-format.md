# Experimental scoped object files

The original synchronous object-storage crate encodes, verifies, publishes and
inspects private binary objects. This is native filesystem support, not an HTTP
upload service, user authorization or a complete object store. Existing database,
WAL and root backup bytes do not change; objects are not included in root backups.

## Exact v1 envelope

All integers are little-endian. Header size is 96 bytes. Payload size is 0 through
8 MiB inclusive. Total length must equal 96 plus the declared payload length.
Unknown versions, flags, reserved bytes, trailing bytes and truncation fail closed.
Distinct project/object identifier types hold 16 opaque bytes; text accepts exactly
32 lowercase hexadecimal characters.

| Offset | Bytes | Value |
| --- | ---: | --- |
| 0 | 8 | `EMILYOBJ` |
| 8 | 2 | Version 1 |
| 10 | 2 | Flags, zero |
| 12 | 4 | Header size, 96 |
| 16 | 16 | Expected project identifier |
| 32 | 16 | Expected object identifier |
| 48 | 8 | Payload length, at most 8 MiB |
| 56 | 32 | SHA-256 of payload only |
| 88 | 4 | Reserved, zero |
| 92 | 4 | IEEE CRC32 of the preceding 92 header bytes |
| 96 | Declared length | Exact payload |

`verify` checks the complete image before exposing a borrowed payload with a
private constructor. Expected scope comes from trusted caller context; header
fields do not grant access. A writer can generate valid new hashes, so checksums
are not an authenticated signature, MAC or encryption. Debug hides payloads.
This version remains experimental. Incompatible changes need a new version,
documentation and explicit separate-output conversion tests, never in-place
repair or upgrade of unknown input.

## Native publication and inspection

`encode` produces bounded bytes only. `publish_file` reuses the original storage
owned stage: retain destination directory, exclusively create random 0600 file,
write/fsync, reread exact bytes, check identity, Linux no-replace rename, fsync
retained parent and check selection. Final full format inspection precedes a
success report. Existing names, including symlinks, cannot be overwritten.
Concurrent attempts select at most one complete image.

Before selection, failure leaves no new target. After rename, sync uncertainty or
failed final inspection reports `PublicationUnknown`: selection may already exist.
Inspect explicitly rather than assuming rollback or blindly retrying. Killed
pre-selection writers can leave private random staging files; cleanup/inventory
policy is separate work. Paths are trusted operator input, not HTTP filenames.
Publication refuses a final-parent symlink; selected ancestors remain trusted.

`inspect_file` opens readonly with final no-follow/nonblocking flags, requires a
regular singly linked 0600 or 0400 file, bounds size before allocation/read,
verifies all bytes and rechecks identity/size/timestamps/mode/link admission.
It never writes or repairs. This is a checked snapshot, not a retained lease or
protection from a hostile filesystem owner changing the file afterward. Same-UID
administrators remain trusted.

Offline `emilybase object-verify PATH PROJECT OBJECT` validates identifier shape
before opening input. Success prints only format, payload byte count and SHA-256
JSON. Failure output is metadata-only; stdout failure leaves source unchanged.
It never infers trusted scope from the untrusted header.

## Verification boundary

Tests use an independent Python struct/hashlib/zlib byte vector, every prefix and
byte corruption of a synthetic image, resealed invalid headers, foreign scope,
8 MiB bounds, generated binary payloads and arbitrary bytes. Native/actual CLI
cases refuse aliases, FIFO, public permissions, oversized and corrupt files.
Shared storage fault cases cover file/parent sync failures, altered staging and
collisions. Competing writers and received native publication followed by process
kill/reinspection are checked. Process kill is not hardware power loss. The fuzz
target exercises the real bounded decoder, not HTTP or filesystem adversaries.
See [ADR0117](adr/0117-scoped-object-files.md).

HTTP upload/download, authenticated project lookup, object policies, quotas,
inventory/delete, signed URLs, object backup integration, larger streaming objects
and production acceptance remain open.
