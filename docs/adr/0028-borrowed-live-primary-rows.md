# ADR 0028: borrowed validated rows in primary order

Status: accepted for Snapshot reads; SQL adoption follows separately.

## Context

Double-ended B+ cursors now yield borrowed keys and opaque pointers. Table text
keys still admit 3072 bytes, while tree keys admit 256. Existing snapshot range
APIs clone result rows; SQL materializes a source before filtering/ordering.
Limited reads need a bounded transient row image, preserving long keys, physical
liveness and immutable historical views.

## Decision

Expose `Snapshot::primary_rows(name, lower, upper)` as a synchronous iterator of
`Result<&Row>`, inclusive lower/exclusive upper. Bind schema/table and validate
both key types/sizes before handling contradictions or empty input. The cursor
borrows only the snapshot: temporary names and bounds can drop after construction.

Use the authoritative live ordered map to merge short and excluded long keys.
When bounds fit the tree, consume a borrowed B+ interval in the same direction
for every eligible live key; missing/extra/mismatched entries or obsolete pointers
fail. On exhaustion check that no tree entry remains. With longer text bounds,
verify eligible keys through original point lookup. Long keys resolve their
current physical image through the existing table/key/page/slot/fingerprint and
decoded-row checks. Every returned row is the actual stored live reference.

Schema key validation now checks borrowed type/length before materializing any
owned value. A separately reproduced oversized-input failure showed that cloning
before size rejection could abort under a process memory cap. Preserve the same
error ordering and valid integer/text semantics, with no input-sized copy.

Support either direction and arbitrary interleaving. On exhaustion/error retire
both ends; a typed failure appears once. Immutable snapshot borrowing prevents
concurrent mutation of that view; clones may change independently. This is not
MVCC, a commit acknowledgment or an authorization boundary. The caller determines
whether its snapshot is committed, staged or detached.

Full imported tree validation retains complete topology, live-key/pointer and
physical-image verification, but compares a streamed key sequence instead of a
whole allocated vector. Existing allocating integer/text range APIs remain as
before; their bounded verification semantics are not silently weakened.

## Consequences and verification

No result row vector is constructed before caller consumption. Lazy derived-tree
construction remains bounded by table capacity and excluded from row-execution
work accounting. Per-row physical validation still decodes a bounded transient
event; this is not a zero-allocation API. A partial read validates consumed rows,
not unread physical images; public cache imports still validate the entire tree
before admission. Short smoke fuzzing is not a wider corruption/power-loss audit.

Tests compare independent ordered rows, both key types, NUL/Unicode/long keys,
3072-byte bounds, nullable/non-leading primary columns, mixed ends, temporary
bound lifetimes, fused failures and actual row reference identity. Ten thousand
wide rows and eight concurrent cold pure readers execute. Clones retain old
images after mutation. Real staged transactions, rollback/abort, reopen, adopted
caches and both-version verified backup/restore preserve committed history;
restored writes remain independent. Raw and integrity-repaired WAL ASan inputs
compare borrowed reads against existing validated rows and replayed snapshots.

A Linux child regression holds a synthetic 128-MiB text key, caps address/data
growth to eight MiB and disables core dumps. The original validator aborts on a
128-MiB clone; the borrowed validator returns the existing ValueSize error and
still accepts 3072-byte Unicode/rejects wrong key types. Only the child changes
resource limits. Its rustix process support is a dev dependency, not a new
runtime engine or service.

Persisted formats/HTTP contracts remain unchanged. SQL ordering/early filtered
LIMIT, independently durable table index WAL, MVCC and production acceptance
remain pending.
