# ADR 0025: bounded optional primary-cache startup after WAL replay

Status: accepted for experimental automatic adoption.

## Context

Private table-cache files previously required explicit loading. Recovery must
continue to verify the mandatory WAL first, and optional damaged/old/foreign
images must not prevent reads or mask missing acknowledged history. Multiple
files need a whole-database input-work bound as well as per-file limits.

## Decision

After successful WAL open and complete relational replay, attempt caches only
for current validated table IDs. Use the same private descriptor-relative reader
and full identity/history/live-key/pointer validation. Rejected/absent/budget-skipped
files leave derived cells reconstructible from authoritative relational state.
Opening and warming write no cache files or WAL.

Reserve the metadata length plus a one-byte growth probe before each read from a
16-MiB whole-database budget. Read at most that reservation, reject changed lengths,
and charge the reservation even on later read/parse errors. A growing file cannot
expand work beyond its reservation. Oversized/unsafe paths are rejected before
reading. Continue after a skipped large file so later small valid images can load.
The budget bounds cache input work, not all recovery allocation or disk latency.

Expose startup counts only: loaded, missing, rejected, skipped, bytes_budgeted.
They describe that owner's startup, not freshness after later writes. Explicit
warm returns a separate current report. The CLI provides primary-index-cache-status;
HTTP contract and credentials remain unchanged.

Cache the exact page-history SHA-256 in immutable Snapshot clones. Accepted
events always replace the digest cell; failed events and cache installation do
not. Cold historical branches cannot initialize a changed branch's digest.
This avoids hashing the same complete history once per table image. It is not a
credential, logical-data hash, or persistent new format.

## Consequences

WAL loss/damage remains fatal even beside a complete valid cache. Cache fallback
preserves access to recovered acknowledged rows; no automatic cache refresh is
part of commit acknowledgment. Restored databases work without sidecars. An
explicitly copied matching image can load into a verified restore clone until
its history diverges. Retired table IDs and orphan staging are never adopted.

Operator-selected ancestors remain trusted. Later
[ADR 0036](0036-owned-database-initialization.md) deliberately refuses final managed
directory aliases; its no-follow admission precedes optional warmup. An unsafe
regular cache directory makes
caches rejected, while raw WAL-only behavior is retained. Project directory
authorization/ownership checks still happen before scoped execution; warming does
not grant scope. Private host-owner/privileged attackers, physical power loss,
automatic housekeeping, durable index WAL and wider load/security remain open.
No startup speed claim is made without dedicated benchmarks.
