# ADR 0011: Original bounded SQL parser and execution boundary

Status: lexer/parser/AST implemented and tested; planner/executor pending.

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

## Next execution boundary

Resolve all column references against the selected schema, reject ambiguous names,
bind typed values separately, and implement explicit SQL null/boolean semantics.
Use bounded scans and a bounded join strategy before introducing durable indexes.
Execute writes through managed WAL transactions, never legacy raw writes.
Failed scripts must discard their entire staged state; output must be returned
only after successful commit, with unknown commit outcomes reported unchanged.
Transaction control is confined to a complete submitted script rather than an
unbounded interactive session. These execution requirements remain pending.

## Validation

Deterministic syntax/AST/boundary/secret-echo tests and two 64-case properties
exercise arbitrary Unicode input, truncation, escaped text, integer extremes and
separate parameter references. A regression reproduces an overdeep tree assembled
from flat AND followed by OR before the fix. A bounded ASan target covers raw SQL
seeded with valid synthetic DDL, CRUD, predicates, joins and transaction commands.
Parsing does not close query execution, durable index, isolation or security gates.
