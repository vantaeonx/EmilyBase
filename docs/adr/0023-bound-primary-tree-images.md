# ADR 0023: primary-tree images bound to acknowledged relational history

Status: accepted for explicit experimental library cache images.

## Context

The original standalone EBIF snapshot proves tree integrity but cannot establish
which managed table/history its pointers describe. A foreign or old intact tree
must not become the current lookup cache. Current relational WAL recovery remains
the source of truth, with derived trees reconstructed independently.

## Decision

Wrap a complete stable-ID EBIF tree in bounded EBTI version 1. Bind the persistent
WAL database ID, nonzero table ID, acknowledged transaction number and a streaming
SHA-256 of every exact relational page. Protect the outer header with CRC32 and
payload with SHA-256; require the nested revision to match the transaction.

Separate structural inspection from managed verification. Managed verification
requires all binding fields, complete eligible live keys and every actual row
page/slot/image before replacing a single derived cache cell. It does not clone
the complete relational state. Export validates the same projection. Failure
leaves data, WAL and previous cells unchanged. No file publication or automatic
recovery adoption is added in this increment.

Keep full table-key compatibility: integers and text up to 256 bytes use the
tree; longer text remains readable through the existing map. Capacity remains
10000 admitted rows; stable copies of full dense trees use 768 pages.

## Consequences

Existing stored formats remain unchanged. A sibling-table commit invalidates a
previous image even if its tree is still logically correct: coarse whole-history
binding is deliberately easy to verify. Rollback/no-op, current checkpoint,
baseline compaction and verified restore retain a matching image. Future vacuum
can discard it safely. Restore clones retain the database ID, as existing backup
semantics require; subsequent divergent history makes an old image obsolete.

Images contain user primary keys and require private storage. Checksums and IDs
do not grant authorization. Structural inspection must never install a tree.
The pure Snapshot API needs caller-selected scope; the managed Database wrapper
enforces persistent binding. A future private sidecar publisher must preserve
ownership, bounded reads, no-clobber publication and optional-cache failure rules.
Atomic durable table/index allocation and WAL replay remain separate open gates.
