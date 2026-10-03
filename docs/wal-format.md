# Experimental redo journal, versions 1 and 2

## Retained journal, version 1

All integers are little-endian. No existing database engine is involved.
The page-file version remains 1; the journal has its own independent version.

The 64-byte header contains `EMILYWAL` (0..8), version u16 (8..10),
page size u16 (10..12), frame size u32 (12..16), a nonzero 16-byte
database identity (16..32), zero reserved bytes (32..60), and CRC32 (60..64).

Each frame is 4160 bytes: `EWFR` (0..4), version u16 (4..6), kind byte
(6, page=1 or commit=2), zero reserved byte (7), transaction u64 (8..16),
global sequence u64 (16..24), page ID u64 (24..32), payload length u32
(32..36), zero reserved bytes (36..60), payload (60..4156), CRC32 (4156..4160).
Page payloads are exactly 4096 bytes and independently validated by storage.
Commit payloads contain page count u32 and CRC32 of all encoded page frames
in the transaction, followed by zeros. Commit page ID is zero.

Transaction IDs begin at 1 and advance only after commit; frame sequences also
begin at 1. Each transaction has 1..256 pages in strictly increasing page-ID order.
The whole journal is capped at 64 MiB before allocating input buffers.

Only a valid commit closes a transaction. Acknowledgment follows `sync_all` of
the commit and preceding page frames. Recovery exposes complete transactions;
valid page frames without a commit and a final partial frame are ignored.
A full corrupt frame is an error even at EOF: treating arbitrary checksum
failures as interrupted writes could silently discard acknowledged data.
Opening never truncates input. A subsequent append removes and syncs the ignored
tail before reusing transaction/sequence identifiers.

An I/O error while writing or syncing a commit has an **unknown outcome**.
The handle refuses further work; reopen and inspect that transaction ID before
retrying. A failed response does not prove rollback. This differs from a batch
for which no commit record was ever written.

CRC32 detects accidental damage, not malicious modification. Locks are advisory.
Durability assumes a local filesystem and hardware honoring synchronization.
Deletion or truncation of an already synced log by an administrator is outside
the crash model. Media redundancy and background segment retirement are pending.

## Self-contained baseline, version 2

Explicit compaction keeps the header/frame sizes and changes the WAL version to 2.
Header bytes 32..40 hold the original last committed transaction ID; bytes 40..44
hold the nonzero baseline page count. Bytes 44..60 remain zero. The transaction
anchor must be in 1..u64::MAX; every frame version must match the header.

The first frames are baseline pages (kind 3) with contiguous IDs starting at 1,
followed by a baseline commit (kind 4). All use the anchor transaction ID and
sequences starting at 1. The baseline commit binds the declared page count and
CRC32 of every complete encoded baseline page frame. It uses the same eight-byte
count/digest payload and zero padding as a normal commit. Baselines may exceed
256 pages but must fit the 64 MiB total WAL bound. This does not relax the normal
transaction limit of 256 pages.

An absent, partial, corrupt or mismatched baseline commit is always an error.
No incomplete baseline may be treated as an ignored tail. After that commit,
normal page/commit frames (kinds 1/2, version 2) continue at anchor + 1 and the
next sequence number. Their final incomplete transaction follows the existing
tail rules. A second baseline, mixed versions and noncontiguous baseline page IDs
are rejected. Opening is read-only; incomplete input is preserved.

Relational replay validates all baseline history, then strictly appends normal
transactions without allowing old event rewrites. The selected `redo.wal` alone
restores the database; checkpoint files cannot authorize fallback. Compaction
preserves the original history and table identities rather than vacuuming events.
Version-1 logs remain readable and new databases still start with version 1.
See [ADR 0008](adr/0008-self-contained-journal-compaction.md).
