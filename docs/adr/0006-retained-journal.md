# ADR 0006: Retained authoritative journal for the first transaction engine

Status: accepted; integration follows the independently tested WAL increment.

Use a new managed directory with a mandatory `redo.wal` and an optional
`checkpoint.emily` page snapshot. Its first committed transaction contains the
initialized root page. The journal's random 128-bit identity identifies this
database. Opening validates the journal, reconstructs the latest committed page
images and strictly replays their relational history. No existing table file is
silently upgraded. The version-1 page format remains readable by diagnostics.

Keep the complete journal authoritative and bounded to 64 MiB. Initially a
checkpoint materializes the latest committed pages atomically but does not
retire journal frames. Recovery ignores the disposable checkpoint, so damaged,
missing or partially written checkpoint pages cannot overwrite committed state.
The cost is bounded journal growth and replay from its beginning. At capacity,
refuse another transaction before writing. Rotation requires a later protocol,
versioned durable checkpoint metadata and a crash-tested retirement procedure.

This choice avoids adding mutable durability metadata to the existing immutable
header or accepting a foreign sidecar merely because two generic headers match.
Copying just a legacy page file does not create a managed transaction database.
Copying a stopped managed directory preserves its identity; it is a clone, not
a newly isolated project. Project IDs and authorization are separate future work.

One owner holds the journal lock. A transaction mutably borrows that owner and
stages a bounded state copy plus shared immutable pages. Commit writes changed
full pages and a commit record, syncs the journal, then publishes memory state.
Rollback/drop discard staged memory without logging a commit. No data-file I/O
follows the durable commit point, avoiding a second fallible acknowledgment step.
An I/O failure at the commit point still has an unknown outcome and requires reopen.

This is a deliberate initial limit, not a claim of complete stage-2 acceptance,
efficient checkpoint reuse, backup verification or resistance to failing media.
