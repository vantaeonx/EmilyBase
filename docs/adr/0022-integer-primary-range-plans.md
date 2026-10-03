# ADR 0022: integer primary-key range plans

Status: accepted for bounded integer intervals.

## Context

Derived primary trees already route point equality, but inequalities still scan
every row. Repeated narrow UPDATE/DELETE scripts can spend their work budget on
unrelated rows. Range extraction must preserve null logic, OR/NOT, parameter/type
validation, row ordering, aliases and transaction atomicity at i64 boundaries.

## Decision

Expose a synchronous Snapshot interval API with inclusive lower/exclusive upper
integer bounds and the existing 10000-result cap. It uses original linked B+
leaves, compares bounded returned keys to current live ordered keys, and resolves
each physical pointer/image. Empty/reversed ranges return no rows; schema and
limit validation happen first. This API rejects non-integer primary keys.

After complete schema/parameter/type binding, derive necessary integer primary
inequality bounds from AND conjuncts. Reverse comparisons when the literal is on
the left; intersect multiple bounds. Checked successors normalize > and <=:
id > i64::MAX is empty, while id <= i64::MAX has no upper bound. Never wrap.
Keep the complete predicate for evaluation. Do not extract bounds from OR/NOT,
column comparisons, nonprimary/text keys or joins. Usable equality takes priority.

SELECT, UPDATE and DELETE consume the same interval through validated snapshots.
Sorting/LIMIT, null logic and staged commit/rollback remain unchanged. Explain
adds primary_range to the existing access enum without returning bound values.
OpenAPI and the strict client decoder accept this experimental plan label.

## Consequences

No stored format changes. This is integer range routing, not secondary-index DDL,
text/collation range planning or ordering pushdown. Row maps still supply the
bounded integrity comparison and other scans. Initial derived-cache construction
remains outside the row-execution work counter, as documented in ADR 0021.
Queries still validate missing columns/types/parameters before LIMIT 0 or an empty
interval. The complete script publishes no writes on later failure/rollback.

An execution regression for the legal alias x exposed an existing parser ambiguity:
predicate operands treated every X word as a hex literal. Classify it as a bytes
prefix only when the next token is a string; X/x column names and qualifiers now
work while valid/malformed hex literals retain their checks. Parser and SQL
execution fuzzing were rerun after this repair.
