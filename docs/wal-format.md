# Experimental redo journal, version 1

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
the crash model. Log rotation, segment retirement and media redundancy are pending.
