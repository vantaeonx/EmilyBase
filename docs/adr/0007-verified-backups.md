# ADR 0007: Committed-WAL backups and atomic verified restore

Status: implemented in the library and CLI; process-kill publication boundaries
and competing-destination tests pass. Power loss and upgrades remain open.

The retained WAL is authoritative, so back up its acknowledged prefix rather
than a cache or an unlocked filesystem copy. Store a versioned 128-byte envelope
with database ID, commit boundary, format versions, bounded length, SHA-256 payload
digest and header CRC32. Verification performs full relational replay; matching
file hashes alone do not prove valid database semantics.

Hold exclusive source ownership during export and compare recovered pages with
the visible committed snapshot. Creation rereads the staged file before publishing
it. Restore validates before writing, stages privately, opens the restored engine,
compares exact WAL bytes, syncs a checkpoint and publishes the whole directory.
Existing files, directories (including empty ones) and symlinks remain untouched.

Use RustCrypto's SHA-256 implementation as a normal crypto dependency. Use the
safe rustix filesystem wrapper for Linux no-replace directory rename; the project
continues to forbid unsafe code. These libraries are not storage/database engines.
See [rustix rename API](https://docs.rs/rustix/1.1.5/rustix/fs/fn.renameat_with.html).

Keep format parsing and engine replay independent of file publication. This
supports malformed-input properties, fuzzing, crash barriers and deterministic
verification. The initial implementation uses bounded whole-file buffers and
blocks writers while copying. Streamed/incremental backup and high availability
are later work. Preserve identity intentionally; project isolation is a separate
server concern. See [backup format](../backup-format.md).
