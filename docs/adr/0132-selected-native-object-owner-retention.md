# ADR0132: selected native object owner retention

Status: accepted for experimental synthetic-data use.

## Context

Existing native put and bounded put keep the actual selected inode through their
own result checks, then return metadata and release the descriptor. Reopening the
filename after external caller work can adopt an identical-byte replacement. The
proposed AccountRoot file protocol requires actual selected retention across a
future catalog commit, in addition to current authority and ownership integration.

## Decision

Provide additive put_selected and put_bounded_selected entry points. Return opaque
non-cloneable lifetime-bound SelectedObject/SelectedWrite, containing the actual
selected file and borrowing its original cooperating directory owner. Share the
original checked write paths; retain the original bounded receipt without changing
its meaning. Expose expected metadata and explicit full streamed revalidation,
never raw filesystem handles or detached user capabilities.

Revalidate exact retained bytes, expected scope/ID/hash, marker owner, private/stable
metadata and visible selected inode before/after the final owner observation.
Report failed later checks as uncertain already-published results. Keep old entry
points, format, synchronization, backup/restore and caller authority unchanged.

## Consequences and acceptance

Holding this native guard does not authenticate a user, atomically commit catalog
metadata or reserve a persisted quota. Its expected receipt is not a later complete
inventory lease. Same-UID/native filesystem trust and post-observation races remain.

Require actual selected descriptor identity across caller work, owner lock lifetime,
same-byte replacement despite equal reports, moved/replaced directory paths and
preserved foreign entries. Cover scope/length/hash/mode/link/marker changes before
and after body verification, empty/8 MiB payloads, generated owned-model comparison
and compile-fail tests for both returned owner lifetimes. Run supported toolchains
on frozen hashes; no new parser or crash boundary is claimed by these handles.
See [contract](../selected-object-retention.md).

Executed209 relevant checks per stable/minimum on582 frozen hashes. Eight new
regular cases include64 generated native selections and26 timed native mutation
shapes; two compile-fail cases check both returned owner lifetimes. Existing25
kills reran; no new kill/parser/durability boundary is added. The contract records
full-source, sanitizer and proposed root integration limits explicitly.
