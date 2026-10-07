# Admit projected output before copying it

The SQL executor now checks every selected row against the shared output budget
before cloning its selected fields. This applies to ordinary full sorting, bounded
primary JOIN sorting and both streamed output paths. See
[ADR0060](adr/0060-admit-projected-output-before-copy.md).

A valid SELECT may repeat a large field many times. Previously, the common sorter
built every selected result and only then discovered that its output was too large.
A warmed native regression observed 99388593 requested bytes before reporting the
8-MiB output-limit error. The same query now observes 14210993 bytes, keeps the same
error and releases to zero. Source/candidate data remains separately retained; an
8-MiB output allowance does not imply an 8-MiB process heap.

The private helper borrows a validated candidate, checks every selected index and
sums the existing logical charge: 24 bytes per row, 32 per selected cell and every
selected text/bytes byte. Repeated fields count repeatedly; NULL/empty fields keep
their fixed charge. It admits the sum before copying any output value. Full sorting
projects rows incrementally. All results in one script share the original 8-MiB
allowance. Returned metadata, order, aliases, values/float bits and typed errors
retain their previous behavior. Explicit projections still have at most 64 fields.

Tests cover exact admission boundaries, each read path, original parser/type/empty
binding, old views and independent generated projections. A staged write followed
by excessive output leaves exact WAL/state unchanged in both versions; rollback,
checkpoint/reopen and verified backup/restore remain readable. ASan and complete
suite results appear in [testing](testing.md).

[Native observations](measurements/2026-10-07-projection-admission/operation-peaks.json)
exclude complete fixture/cache construction, allocator overhead/rounding, stacks
and profiler data. The admission is a logical selected-payload bound, not a quota
for metadata, candidate heaps, sources, caches, staging, temporary predicates,
workers, JSON encoding or the whole process. No stored format, grammar, API,
durability acknowledgement or production gate changes.
