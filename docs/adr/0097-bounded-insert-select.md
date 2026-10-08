# ADR0097: Bounded INSERT SELECT through the original query planner

Status: accepted for experimental original SQL and offline migrations.

Schema rebuilds need to copy existing typed data before replacing a table. Add an
original AST/parser INSERT SELECT variant using the existing SELECT grammar, plan,
checked primary/range/join/sort paths and shared script budgets. No new database
record/event, WAL version, engine dependency or implicit cast is required.

Resolve target positions, source columns and exact declared types before scanning,
including empty input/LIMIT0. Omitted target values use existing NULL rules when
actual rows are inserted. Cap the planned SELECT result at remaining transaction
events+one lookahead; user LIMIT/filter/order are preserved on every success. If
lookahead proves excess, refuse the entire script instead of truncating a copy.
Keep query work/output accounting, including internally materialized selected rows.
Finish selection before any insert so self-copy cannot read its own writes. Use
normal typed inserts; duplicate/null/record/budget errors discard the entire owned
script transaction. Shared original256-event limits include preceding statements.

Allow this write form in migrations. Reject reserved ledger targets and FROM/JOIN
ledger sources by parsed names. The final required receipt still consumes an event;
a script filling256 events is discarded if its receipt cannot fit. Document a
bounded explicit copy/drop/create/copy/drop recipe for nullable-column rebuilds.
No ALTER/schema diff/down or large multi-transaction migration is introduced.

Acceptance covers parser compatibility, staged sources, typed/reordered/null fields,
exact signed-zero bits, long UTF-8/NUL keys, empty-input resolution, self-copy, joins,
late collisions, shared capacity, query-work/output limits, independent generated
filter/sort/limit models, real migration rebuilds/receipt limits and actual CLI.
Before/after-ACK migration children and real TCP ACK kills on both routers/WALs
verify recovery without private-history writes. Extend parser/mutation fuzzing to
this grammar and independently checked bounded copies. Production gates stay open.
