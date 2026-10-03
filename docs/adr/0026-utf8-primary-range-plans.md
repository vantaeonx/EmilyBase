# ADR 0026: UTF-8 primary ranges without losing long keys

Status: accepted for the experimental implementation.

## Context

Table text keys admit 3072 bytes; the derived B+ tree admits 256. Routing SQL
inequalities through only the tree would silently exclude valid long keys,
especially when LIMIT truncates a mixed interval. Integer primary ranges already
share one planning path for SELECT and staged UPDATE/DELETE.

## Decision

Use UTF-8 byte order, matching the live ordered key map. The synchronous snapshot
range API takes inclusive lower/exclusive upper bounds. It validates key type,
bound sizes and row limit before returning an empty result. Short bounds traverse
linked B+ leaves, compare the bounded eligible key sequence and resolve every
returned tree pointer against its current physical image. All ordered live keys,
including excluded long keys, define the final rows and LIMIT. Long bounds use
the live map directly, still resolving each returned physical row.

After complete binding, the original SQL planner extracts only necessary AND
comparisons with the primary column and text values. Reverse operands and
intersect multiple bounds. Appending the minimum Unicode scalar NUL gives the
exact least valid string greater than a text value. Normalize `>` and `<=` with
that successor only when the result fits the tree's 256-byte bound; `<` and `>=`
can use the original bound. Longer/unrepresentable constraints remain filters.
Other usable necessary conjuncts may still supply an interval. Equality keeps
priority; OR/NOT, joins and column comparisons retain their existing paths.

The full predicate executes on resulting rows. Preserve null logic, explicit
ORDER BY, alias/type/parameter validation, atomic script rollback and live table
admission. Explain uses the existing `primary_range` contract for either key type.
No file format, HTTP schema or SDK behavior changes.

## Verification and consequences

Tests compare independent ordered-map models, all inequality directions,
reversed operands, empty strings, embedded NUL, Unicode, 256/257/3072-byte edges,
nullable predicates and contradictory intervals. A forged missing/extra/wrong
short cache fails even if a long key would fill LIMIT. A real 10000-key table
preserves mixed ranges, and 64 narrow mutations among 6000 rows fit the existing
execution work limit. Both WAL versions, verified restore, actual CLI and
container HTTP checks execute. The SQL execution ASan target now compares
generated text ranges independently alongside raw parser/executor inputs.

The live map remains the authority for merged output. Tree construction and
integrity comparisons are bounded by table capacity and are not included in the
row-execution work counter; this change makes no throughput claim. Locale
collation, ordering pushdown, independently durable table index pages and
secondary-index DDL remain future work. Recovery, load and security acceptance
gates remain open.
