# ADR 0027: borrowed double-ended index cursors

Status: accepted for the original bounded tree; table/SQL streaming remains next.

## Context

The original range API allocates and copies every returned key. SQL primary
ordering currently materializes and sorts rows before applying LIMIT. Efficient
limited ordered reads need borrowed forward and reverse traversal. EBIX-1 has
only successor links, and changing persisted page bytes requires compatibility
and recovery work unrelated to this read-only foundation.

## Decision

Expose a synchronous cursor borrowing an immutable BPlusTree and owning at most
two validated 256-byte bounds. Yield borrowed keys and copied opaque pointers.
Support inclusive lower/exclusive upper bounds, both directions, arbitrary mixed
end consumption and Iterator/FusedIterator. Crossing, exhaustion or a typed
error retires both ends; errors are yielded once. Validate bounds even for empty
trees or contradictory intervals.

Seek each end through separator binary searches while retaining at most the
existing eight levels of ancestor frames. Step to adjacent subtrees via those
frames, descending to the proper extreme leaf; compare the existing forward leaf
link in either direction. Check visited layouts, monotonicity and entry bounds.
Per-end leaf-transition limits prevent unbounded traversal of malformed internal
structures. Do not add previous-leaf bytes or replace full import validation.

Collect the existing allocating range API from this cursor. Add an explicit
standalone CLI interval reader with a bounded result, preserving full snapshot
validation/ownership and returning key/page/slot JSON only after successful read.
The CLI does not resolve table records or claim authorization through pointers.

## Consequences and verification

Tree mutation cannot coexist with an active borrowed cursor in safe Rust. Clones
retain independent tree state. Cursor setup/storage uses two bounded paths and
small bounds, with no copied page arena or result vector. Existing EBIX/EBIF,
table/WAL and backup bytes remain unchanged.

Tests cover separator and leaf edges, mixed numeric/text order, empty/NUL/Unicode
and maximum text keys, 10000 entries, stable holes, root collapse/reuse, restored
snapshots, alternating ends and independent generated mutation/interval models.
Internal malformed links, values, roots, counts and height fail without panic;
borrowed key addresses equal actual leaf key addresses. An ASan target generates
and mutates mixed trees and compares both directions against an ordered map.
Actual compiled CLI tests cover directions, defaults, limits, private-path and
bad-bound rejection, redaction and unchanged revisions/images.

Borrowed live-row integration, early filtered SQL LIMIT and primary ordering
pushdown follow separately. The cursor is not independently durable table-index
storage, MVCC, an authorization mechanism or a performance benchmark.
