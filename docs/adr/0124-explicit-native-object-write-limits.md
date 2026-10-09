# ADR0124: explicit native object capacity admission

Status: accepted for experimental synthetic-data use.

## Context

Inventory/backup bounds currently refuse an already excessive native directory;
the original immutable put is a filesystem operator primitive and does not enforce
aggregate capacity. A future admitted upload path needs a separate operation that
checks complete current state before allocating/creating a new file, without
mistaking a metadata receipt for permission or a persisted quota policy.

## Decision

Add private-constructor explicit per-call `WriteLimits` within existing inventory
bounds. Pure metadata checks admit only a fresh identity, available name and
aggregate payload bytes. Zero-byte files still charge one name. `put_bounded`
verifies current inventory, admits capacity, builds the complete expected sorted
metadata/digest before staging, rechecks source, reuses original immutable put,
then verifies the complete final inventory. Postselection disagreement preserves
the selected file with an unknown result. Return payload-free checked metadata.

Recompute capacity from checked files on reopen; do not add a volatile counter or
silently rewrite existing over-limit data. Keep original native put explicitly
distinct. Add an offline limited-write CLI with argument validation before stdin
and a smaller configured input bound. No wire/database/object format changes.

## Verification and limits

Final evidence will record both Rust versions, independent map/byte-sum histories,
exact physical count/byte bounds, competing serialized callers, namespace/project
isolation, before/after-selection mutations, real received/unreceived process
kills, bounded unfinished stdin and actual stdout failure.

Limits remain per call and require native filesystem authority. They do not grant
access, reserve global heap, persist administrator policy or automatically guard
the separate unbounded primitive. Observations are not leases against native
administrators. HTTP/file permissions, AccountRoot integration, persisted quotas,
resource/rate limiting and production gates remain open.


Executed148 checks each stable1.99/minimum1.89.0:82 object,42 storage and24
actual CLI, on566 frozen hashes. Sixteen new regular cases include24 independent
capacity histories with up to24 operations, eight competing serialized callers
admitting exactly two, physical128 empty names/64 MiB boundaries, duplicate/zero
limits, retained namespace/project isolation and pre/postselection changes. Four
new process kills distinguish admitted/unselected, selected/unreturned and received
empty/nonempty receipts; sixteen old kills rerun. Actual CLI rejects unfinished
overflow without EOF, rejects invalid arguments before stdin and preserves a
selected empty file on stdout failure.

Workspace/fuzz format/strict lint, stable CLI/server build, minimum workspace build
and all-fuzz compilation pass. No new parser/format or new ASAN run is claimed.
Ignore probes cover native marker, byte/directory stages including private children,
objects and archives. See
[verification](../measurements/2026-10-10-object-write-limits/verification.json).
