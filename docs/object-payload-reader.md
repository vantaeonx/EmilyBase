# Retained native object payload reader

`ProjectDirectory::reader(ObjectId)` fully verifies the existing scoped envelope
through bounded hashing, then retains its actual readonly descriptor and borrows
the original directory owner. It returns ObjectReader with expected scope, length,
hash and original stable private metadata. No payload image is retained. Opening
never creates, adopts or repairs a missing/corrupt/foreign object or marker.

`read_payload` copies at most8192 bytes into caller-owned memory, excludes the
96-byte object header and maintains a payload-relative logical cursor. Before and
after a copy it validates the original owner/marker, stable file length/timestamps,
private regular single-link metadata and original visible inode. Empty reads and
EOF do not skip these checks. Independent readers keep independent positions;
full hashing uses a separate file cursor from positional payload reads.

Any failed read clears the attempted destination prefix, preserves the logical
cursor and poisons this reader. The attempted prefix is the minimum of destination
length,8192 and remaining payload bytes. Bytes after it remain unchanged. A failed
seek or complete verification caused by filesystem state also poisons the reader.
After poisoning, reads, seeks, verify and finish refuse even if an operator repairs
the filesystem. Earlier successfully returned chunks cannot be withdrawn.

`seek_payload` accepts payload-relative Start/Current/End and positions past EOF.
Negative or overflowing relative positions return InvalidSeek without changing
the cursor or poisoning a healthy reader. `verify` hashes the complete retained
envelope, compares the original length/hash and repeats owner/identity checks while
preserving the logical cursor. `finish` consumes the reader after that same complete
verification and returns the expected FileReport. Metadata getters alone confer
no current authorization. Debug exposes only counts/cursor/poison state.

```rust
use emilybase_object_storage::{ObjectId, ProjectDirectory, Result};

fn inspect_chunks(owner: &ProjectDirectory, object: ObjectId) -> Result<()> {
    let mut reader = owner.reader(object)?;
    let mut scratch = [0; 8192];
    while reader.read_payload(&mut scratch)? != 0 {
        // Native caller consumes this verified observation here.
    }
    reader.finish()?;
    Ok(())
}
```

The synchronous API performs blocking filesystem work and must run inside the
server's admitted blocking worker if a future transport uses it. There is no
network timeout, HTTP response completion or whole-process memory quota here.
The8192 limit bounds each copied payload chunk, not caller buffers or operating
system caches. Marker/path checks may allocate small bounded metadata; no claim
of zero total allocation or measured throughput is made.

The contract assumes cooperating immutable-file owners and trusted native
operator/ancestors, as the existing store does. Timestamp/identity observations do
not isolate arbitrary concurrent same-user filesystem writes or sandbox an
administrator. A successful final check is an observation, not a permanent lease
on a later filename. Moving the original directory keeps its namespace; opening
a replacement original path cannot redirect a retained reader.

This is an additive native prerequisite, not user file authority, catalog metadata,
persisted quota, signed downloads, AccountRoot attachment or coordinated backup.
Existing get/inspect/publication/backup/restore APIs and stored formats are unchanged.
See [ADR0134](adr/0134-retained-bounded-object-payload-reader.md) and the still-open
[root integration proposal](adr/0127-proposed-account-root-object-integration.md).

## Verification boundary

The source-bound results are recorded in
[verification.json](measurements/2026-10-10-object-reader/verification.json).
Coverage includes generated slice-model cursor operations, the maximum object,
same-byte new-inode substitution, mutations before/after copy and after full hash,
poisoning, private readonly files, original moved scope, concurrent scoped readers
and a compile-fail owner-lifetime case. No new parser or durable selection is added;
the existing native process-kill tests remain relevant regression coverage.

Stable1.99 and minimum1.89 each passed225 relevant checks:137 object cases, three
owner-lifetime compile-fail doctests,58 storage cases and27 CLI checks. Eleven new
regular cases include64 generated binary slice-model histories with1..47 cursor
operations each, six scoped readers with24 independent read/hash sequences each,
empty/readonly/8 MiB objects, EOF/overflow and irreversible poisoning. Thirteen
mutation shapes refuse before/after copying, after admission hashing and after
explicit revalidation hashing, including identical-byte replacement at a new inode.

All585 frozen source/configuration hashes match after the final checks. Formatting,
strict stable workspace/fuzz lint, builds and minimum all-fuzz compilation exit0.
Existing25 native kill boundaries reran; no new durable boundary or kill is claimed.
The first attempt was interrupted with the surrounding work; incomplete results
are excluded and the relevant commands were rerun to captured successful exits.
The separate immutable24444bc broad run completed1552 stable checks and a full
build; its resumed minimum run is pending and cannot certify this later source.
