# ADR0096: Ordered bounded migration receipts in the original engine

Status: accepted for experimental offline project data databases.

Use the owned staged SQL API to atomically combine schema/data changes with a
normal original-engine receipt. A separate receipt file or second transaction
could diverge after a crash. No new engine/WAL event or external storage dependency
is needed. Add a synchronous migrations workspace crate and explicit CLI stdin
apply/metadata inspection; do not expose a network-held transaction or migrate
private account schemas through this tool.

Require consecutive versions1..128, bounded ASCII labels and the existing bounded
SQL write subset. Reject transaction controls/SELECT and parsed ledger targets.
Hash exact SQL/label/version bytes with domain-separated SHA-256 and length prefixes.
Store a strict v1 conventional table of version/label/digest/commit metadata. Validate
all existing receipts, including canonical increasing non-root commit numbers,
before order/retry decisions. Identical historical retries are read-only; mismatches,
gaps, malformed/empty existing ledgers and skipped versions refuse without repair.

The first ledger creation, complete SQL and receipt use one original256-event
transaction; subsequent write failures abort it. Event/table/row/page/WAL limits
include metadata. ACK follows original WAL sync; ambiguous commit errors propagate
and require reopen/definition check before another attempt. Original receipt commit
numbers are retained across ordinary intervening writes and verified restore.

The reserved name is a convention enforced for migration scripts. Trusted database
owners/service SQL still have full control and can forge/delete metadata; receipts
are not access control, signatures or an independently authenticated audit trail.
No automatic down/schema diff/ALTER, multi-commit migration or production acceptance.

Verification covers first/next/repeated/changed/skipped/late-failed definitions,
128 receipts, metadata event capacity, damaged metadata, exact read-only history,
checkpoint/verified restore, a separate generated version/data model, an independent
digest vector, actual CLI input/ownership/output refusals and controlled child kills
before commit/after ACK on both WAL versions. Extend the existing pure SQL parser
fuzzer to admit bounded migration definitions. Broader upgrade/crash/security and
whole-process reservations remain open.
