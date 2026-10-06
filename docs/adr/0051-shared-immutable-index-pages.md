# ADR 0051: share immutable decoded index pages

Status: accepted for the original in-memory index arena. No runtime format change.

## Context

ADR0050 removes redundant physical image sets, but cloning an index still clones
every decoded page/key vector. Staged writes, delta application and retained model
views therefore copy unchanged arenas. Before changing ownership, the optional
10000-row text diagnostic fails its128-KiB clone guard at3282472 requested bytes.

## Decision

Keep the bounded page map and store each private decoded page behind Arc. A tree
clone owns a distinct bounded map of shared immutable page handles. Public page
constructors/decoders and owned physical images retain their existing APIs. No
mutable page reference or ownership handle escapes the arena.

Mutations copy a visited page into private working state and replace its handle
only if its complete value changes. Equality includes keys, row pointers, child
links and the leaf successor. Unchanged visited branches retain their old handle.
Splits allocate new pages; rotations/merges detach participating pages; retired
IDs disappear only from the new map. Reusing a retired ID never overwrites a page
still owned by a historical snapshot. Dense deletion remaps IDs/links in private
owned values, retaining a handle only when every field remains equal.

Transactional insertion/deletion continue to stage a private map and publish only
after existing checks succeed. Delta application shares unchanged base handles,
decodes changed pages privately, then validates the complete candidate including
topology and original wire checks. No upsert/retirement is published on failure.
Last-owner release frees replaced/retired pages; immutable readers can outlive the
writer and its serialized plan. Independent clones can be changed in separate
threads; this does not coordinate two commits to the same logical database.

## Verification and limits

Private owner/key pointer identity and Weak release tests distinguish actual
sharing from value-equivalent copies. Full-capacity text, split/merge/root collapse,
ID reuse, dense remapping, real exhausted-arena failure and independent mixed-key
accepted/discarded histories execute. Model tests check root-only and one-page
prepare/encoded replay, stale predecessor refusal and four real scoped workers.
Existing frozen EBIF/EBIX fixtures and complete corruption admission remain active.

The isolated feature-only diagnostic observes clone26304 and apply55912 requested
bytes for its full-text fixture, with operation-local current bytes returning to
zero. A same-config whole-model experiment retains four10000-row projects and
actually replays four workers. Its preserved reports distinguish pre-stream,
post-stream and shared-page implementations; instrumented elapsed times are not
throughput claims. See [observations](../shared-index-pages.md).

Map nodes, Arc headers/reference counters, visited-page scratch, decoded wire
inputs, output vectors and retained historical generations still allocate. Dense
renumbering can detach many pages. Sharing is not a numeric heap/RSS/stack quota,
nor a guarantee that allocation can never abort the process. Existing EnvelopePool
admits serialized bytes only. Runtime cache/model/transient/worker reservation,
cross-version durability and the combined table-index writer remain separate open
gates. WAL1/2, file versions, backups and acknowledged transactions are unchanged.
