# Experimental project-registry backup format 1

Linux/local-filesystem implementation. Public source code does not publish data;
archives contain private plaintext project data and current key digests. Keep them
outside Git. The database master key is not part of the registry and is excluded.
This is independent of the unchanged single-database EMILYBAK version 1.

All integers are unsigned little-endian. Whole-file size is 128 bytes through
128 MiB inclusive; project count is 0..128. Exact lengths and zero reserved bytes
are mandatory. Unknown versions fail; no implicit conversion or staging adoption.

| Header offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII EMILYREG |
| 8 | 2 | version 1 |
| 10 | 2 | header size 128 |
| 12 | 4 | project count |
| 16 | 8 | payload length, exactly file length minus 128 |
| 24 | 32 | SHA-256 of the complete payload |
| 56 | 68 | zero reserved bytes |
| 124 | 4 | CRC32 of bytes 0..123 |

Payload records are sorted by strictly increasing 32-character lowercase-hex
project ID. Duplicate project IDs and duplicate nested database identities fail.
Display names may repeat and never select paths.

| Entry offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 32 | project ID, UTF-8 ASCII |
| 32 | 4 | canonical metadata length, 1..4096 |
| 36 | 4 | zero reserved bytes |
| 40 | 8 | nested EMILYBAK archive length |
| 48 | metadata length | exact canonical project.json envelope |
| following | archive length | verified complete single-database backup |

Metadata must re-encode to exactly the stored bytes and match the entry identity.
It preserves the display name, current SHA-256 key digest and nonzero rotation
epoch. The nested archive must pass its own CRC, SHA, identity/transaction checks
and complete relational replay, with no uncommitted tail. WAL versions 1 and 2
can coexist across projects. There is no trailing data.

Verification reports public project metadata, database identities, transaction
numbers and counts; it never returns digests or rows. CLI output is counts only.
CRC/SHA integrity is not a signature or encryption. Existing plaintext credentials
are neither regenerated nor restored from digests: privately retain the current
project keys, or rotate them through the administrator API after restoring.

## Capture and restore

Stop the server and use disposable synthetic data during development:

```sh
cargo run --locked -p emilybase-cli -- projects-backup /tmp/projects /tmp/projects.backup
cargo run --locked -p emilybase-cli -- projects-backup-verify /tmp/projects.backup
cargo run --locked -p emilybase-cli -- projects-restore /tmp/projects.backup /tmp/restored-projects
```

The source must exist and be exclusively owned. Outstanding capabilities or
database writers prevent capture. Backup targets must be outside the registry.
Missing sources, unsafe files, unknown committed entries, changed metadata or
corrupt journals fail without publishing an archive. Abandoned creation staging
and disposable caches are omitted. The initial conservative aggregate bound uses
physical WAL lengths, so abandoned tails can also cause size-limit refusal.

Archives must be private, regular, single-link files; symlinks, hard-link aliases,
broad permissions and oversized reads fail. Publication uses a private synced
temporary file, exact readback/replay, Linux no-replace rename and parent sync.
Restore first validates the entire input, writes private project/data directories,
syncs journals, replays databases, regenerates checkpoints and compares a fresh
whole-registry image before publication. Existing files, directories and symlinks
remain untouched. Selected output is complete; leftover staging is not adopted.
After publication uncertainty, inspect the destination before retrying.

This backs up the currently implemented project registry. Unrelated standalone
indexes and future objects/user-session components require separate formats.
Archive version 1 remains experimental. Wider crash/fault, physical-power-loss,
streaming, encryption, upgrade and security/load acceptance remain open.
