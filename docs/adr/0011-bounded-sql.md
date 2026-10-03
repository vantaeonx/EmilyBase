# ADR 0011: Original bounded SQL parser and execution boundary

Status: lexer/parser/AST and managed planner/executor implemented and tested.

## Decision

Write original lexer and recursive-descent parser for the documented subset.
Use typed catalog values and schemas; keep parameters as one-based AST references
instead of interpolating their contents into SQL. Bound bytes, tokens, statements,
columns, tuples, literal bytes, parameter positions and expression depth. Validate
combined flat boolean chains as well as nesting, so their left-associated ASTs
cannot bypass the depth bound. Errors identify offsets/categories, not SQL values.

Preserve existing case-sensitive catalog names while treating keywords without
ASCII case sensitivity. Support only the types and constraints the storage layer
can validate. Reject unsupported syntax rather than accepting nonfunctional nodes.
An AST is an inspectable parser result, not a schema-resolved or executable plan.

## Execution boundary

Resolve all column references against the selected schema, reject ambiguous names,
bind typed values separately, and implement explicit SQL null/boolean semantics.
Use bounded scans and a bounded join strategy before introducing durable indexes.
Execute writes through managed WAL transactions, never legacy raw writes.
Failed scripts must discard their entire staged state; output must be returned
only after successful commit, with unknown commit outcomes reported unchanged.
Transaction control is confined to a complete submitted script rather than an
unbounded interactive session. Every script is one transaction; rollback reports
may include staged reads but never claim a commit. The planner uses existing
table primary-key lookup for a usable equality conjunct, scans otherwise, and a
bounded nested loop for one inner join. Work, intermediate rows and retained output
are capped. Sorting and three-valued null logic are explicit in the SQL document.
No durable index pages or file-format changes accompany SQL execution.

## Validation

Deterministic syntax/AST/boundary/secret-echo tests and two 64-case properties
exercise arbitrary Unicode input, truncation, escaped text, integer extremes and
separate parameter references. A regression reproduces an overdeep tree assembled
from flat AND followed by OR before the fix. A bounded ASan target covers raw SQL
seeded with valid synthetic DDL, CRUD, predicates, joins and transaction commands.
Execution tests cover staged rollback on semantic/binding/type/capacity errors,
null truth tables, ordering, joins, row counts, resolution on empty/LIMIT-0 input,
separate binding injection, primary-key/scan/join plans, checkpoint/compaction reopen,
independent generated CRUD histories and the actual CLI. Broader crash/fault/fuzz
campaigns, durable indexes, project isolation and security gates remain open.
