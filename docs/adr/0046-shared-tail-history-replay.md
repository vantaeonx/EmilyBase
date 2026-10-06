# ADR 0046: replay only a canonical history append

Status: accepted for synchronous memory replay. No new durable writer or quota.

## Context

ADR 0045 measured 573246472 additional requested live bytes while replaying four
long-key 10000-row states from only 16384 changed image-body bytes. Reconstructing
all prior pages/events copied every unchanged live body and location map. Existing
copy-on-write snapshots can instead apply validated new records over the base.

## Decision

Add Snapshot::replay_append_pages over an already validated immutable base and
borrowed original Page objects. An empty input shares the base. Nonempty input
is bounded to 256 changed pages and 256 new records independently. It may extend
the last page and/or append contiguous pages. Old slots remain byte-identical;
empty/deleted slots, earlier rewrites, duplicate/order/gap errors and count overflow
refuse before copying the candidate. Complete history retains its existing bound.

Clone the base's immutable handles, decode only the newly added slots and apply
them through the original catalog/event/location/derived-index rules. Table/map
structures detach only where written; unchanged bodies, keys and old pages stay
shared. Any later failure drops the private candidate. Compare every changed
canonical image and the final page count against normal event placement before
returning. A syntactically valid event cannot force a premature new page while it
would fit the prior tail. This is a new in-memory append contract, not a changed
stored page/WAL decoder or a replacement for full recovery from an unknown base.

Physical ImagePlan replay still validates exact database/base/adjacent transaction,
typed addresses and raw checksums before using this append. It still applies
original tree deltas, validates complete row coverage/current physical pointers
and matches the complete expected next fingerprint. Public plan error categories
for committed rewrites and earlier/gapped pages are preserved. Standalone append
does not supply database ownership, a transaction fence or durable acknowledgment.

## Verification and observed consequences

The first regression test fails against full reconstruction because unchanged
rows/pages are copied. It now passes while changed rows detach and the base stays
unchanged. Independent 48-case row-operation properties compare append, complete
history replay and a separate map. Tests cover abort after an earlier valid event,
canonical placement, malformed/root events, gaps/deleted slots/old rewrites,
non-first text keys, drop/recreate and exact 256-page/record bounds. Real full
10000-live-row, 128-table and 100000-event states exercise refusal/reclamation.

The same optimized Linux/Rust/dhat four-long-model workload now adds 6941800
requested live bytes at replay, returning exactly to plans-built on release.
Global requested peak is 588108552 bytes versus 1154413224 previously. Instrumented
maximum RSS is 623460 KiB versus 1242644. These observations are not a universal
bound or a throughput comparison; historical reports remain unchanged.

Map/handle detachment, decoded page buffers, original tree-delta temporary
envelopes, caller-owned outputs and overlapping workers still need numeric byte
reservations. The optional lifetime pool does not expose raw append/plan/replay
outputs. No new WAL, real projects or production gate is enabled. See
[shared replay observations](../shared-history-replay.md).
