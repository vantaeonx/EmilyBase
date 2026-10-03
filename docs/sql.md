# Experimental SQL subset

The original `query` crate provides a lexer, typed AST, parser, schema-resolved
plans and bounded execution through managed transactions. Parser acceptance alone
does not guarantee that schemas, names or row values are valid in a database;
the planner/executor validate them before reading or publishing results.
No PostgreSQL compatibility is promised.

## Accepted syntax

```sql
CREATE TABLE items (id INTEGER PRIMARY KEY, title TEXT, active BOOLEAN NOT NULL);
CREATE TABLE labels (id INT, owner INTEGER NOT NULL, title TEXT, PRIMARY KEY (id));
INSERT INTO items (id, title, active) VALUES (1, 'l''été', TRUE), ($1, $2, FALSE);
SELECT title AS label FROM items WHERE NOT active OR id >= $1
  ORDER BY title ASC NULLS LAST, id DESC LIMIT 20;
SELECT a.id, b.title FROM items AS a INNER JOIN labels AS b ON a.id = b.owner
  WHERE b.title IS NOT NULL ORDER BY a.id LIMIT $1;
UPDATE items SET title = $1, active = TRUE WHERE id = $2;
DELETE FROM items WHERE title IS NULL;
DROP TABLE items;
BEGIN TRANSACTION;
COMMIT;
ROLLBACK;
```

Keywords ignore ASCII case. Identifiers preserve case and use existing catalog
rules: 1..63 ASCII letters/digits/underscores, first character a letter/underscore.
Double-quoted identifiers must still satisfy those rules. Aliases require `AS`.
`SELECT *` means the complete row; otherwise projections are column references,
optionally qualified by a table/alias and renamed by `AS`. There is at most one
`JOIN`/`INNER JOIN` with `ON`; other join types and chained joins are unsupported.

Each table requires one integer/text primary key, inline or single-column table
constraint. Other columns are nullable unless `NOT NULL`. Types: `INTEGER`/`INT`/
`BIGINT`, `TEXT`, `BOOLEAN`/`BOOL`, `FLOAT`/`REAL`/`DOUBLE`, `BYTES`/`BLOB`.
No defaults, generated values, foreign keys or composite keys.

Literals are signed-i64 integers, finite-f64 decimal/exponent values, booleans,
`NULL`, single-quoted UTF-8 text with doubled quote escaping, and `X'00ff'` bytes.
Backslashes have no escape meaning. Numbered parameters `$1`..`$256` are AST
references to a separate binding array, never substituted into the SQL string.
Bindings use catalog values with strict types; there are no implicit casts.

Predicates support column/literal/parameter operands, `=`, `<>`/`!=`, `<`, `<=`,
`>`, `>=`, `IS [NOT] NULL`, boolean operands, parentheses, `NOT`, `AND`, `OR`.
Precedence is NOT, then AND, then OR. UPDATE assignments and INSERT values accept
literals/parameters only. LIMIT accepts an integer 0..10000 or a parameter whose
value will need executor validation. ORDER BY uses source columns and optional
ASC/DESC and NULLS FIRST/LAST. Default null placement is LAST in both directions.
Ties retain primary-key scan/join order. Text sorts by UTF-8 bytes without collation
or normalization. ORDER BY names source columns, not output aliases.

Statements require separating semicolons; the final semicolon is optional.
Empty statements/scripts are rejected. Line `--` and non-nested `/* */` comments
are accepted outside quoted strings. Comments have no runtime behavior.

## Execution and transactions

Every submitted script is one atomic managed transaction, even without BEGIN.
Explicit control requires BEGIN first and COMMIT/ROLLBACK last, with no nested
or intermediate control statements. There is no interactive transaction session.
The complete script is parsed before staging. Any semantic, binding, execution
or write error discards all its staged changes, including earlier statements.
Commit uses the existing WAL sync/unknown-outcome protocol. Successful read-only
scripts preserve the transaction number. WAL/file format bytes are unchanged.

`execute` returns a report with transaction number, `committed` and per-statement
results. SELECT has column labels and catalog-typed rows. INSERT/UPDATE/DELETE have
an affected count; DDL has no row results. ROLLBACK returns `committed:false` and
may include reads of discarded staged data; these are not commit acknowledgments.
Updating the primary-key column is unsupported. Named INSERT columns may omit
nullable fields, filled with NULL. Duplicate named columns/assignments fail.

Expressions use three-valued boolean logic: comparison with NULL yields unknown,
only TRUE passes WHERE/ON, and IS [NOT] NULL is definite. Type errors and unknown/
ambiguous columns are checked even on empty input or LIMIT 0. An alias hides its
original table qualifier; self joins require distinct aliases.

SELECT and filtered UPDATE/DELETE select direct primary-key lookup for a usable equality conjunct;
otherwise it selects a usable primary range or scans. Joins use a bounded nested loop. `explain` resolves one SELECT
without reading rows. Eligible point lookups route through the original derived
B+ tree, then validate their live page/slot/image. Text keys over 256 bytes retain
the map path, through the existing 3072-byte limit. Scans/joins keep current row
maps. No durable index pages or secondary-index DDL are enabled. Joins with large
products can fail their work bound. Initial derived-cache construction is bounded
by table capacity and is outside the row-execution work counter; no throughput
claim is made. See [ADR 0021](adr/0021-derived-primary-key-trees.md).

For integer primary keys, necessary AND inequalities (`<`, `<=`, `>`, `>=`)
select a `primary_range` plan for SELECT and filtered UPDATE/DELETE. Reversed
operands and multiple intersecting bounds are supported without i64 overflow.
The complete filter still executes. OR/NOT, joins and column comparisons
retain their earlier paths; equality takes priority. Empty/contradictory
intervals still validate every field/type/parameter before returning rows.
See [ADR 0022](adr/0022-integer-primary-range-plans.md).

Text primary keys support the same necessary AND inequalities in UTF-8 byte
order, without locale or case folding. Inclusive lower/exclusive upper bounds
can use at most 256 bytes. Strict lower/inclusive upper normalization appends a
NUL scalar: `s + '\0'` is the smallest valid string greater than `s`, including
empty strings and strings already containing NUL. This is usable only when the
normalized bound fits 256 bytes. Longer or unrepresentable literals remain
filters/scans; another usable necessary conjunct can still supply the range.
Every predicate is fully bound before plan extraction, even with LIMIT 0.

The snapshot API validates the short-key tree interval and its live pointers,
then resolves ordered live keys including long keys through the 3072-byte table
limit. LIMIT follows this merge, so a long key preceding a short tree entry
cannot disappear or change ordering. Direct snapshot ranges accept long bounds
through 3072 bytes using the ordered live map. The integrity comparisons and
derived-tree build remain bounded by table capacity, outside the row-execution
work counter. SELECT still applies its complete filter and explicit ORDER BY;
UPDATE/DELETE use the same range on their staged snapshot. Durable index WAL,
secondary DDL and ordering pushdown remain pending. See
[ADR 0026](adr/0026-utf8-primary-range-plans.md).

The library's `query(snapshot, sql, parameters)` evaluates exactly one SELECT
without file access or mutations, allowing reads of a detached validated snapshot.
The caller determines its committed/staged provenance; it is not a commit ACK.
Write/control/multiple-statement scripts are rejected by this read-only entry point.
`execute` remains the managed transaction entry point. Predicate evaluation borrows
validated row/literal values instead of copying their text/blob contents per node.

```sh
cargo run -p emilybase-cli -- sql /tmp/emilybase-demo 'SELECT * FROM items WHERE id=$1' --parameters '[{"type":"integer","value":7}]'
cargo run -p emilybase-cli -- sql /tmp/emilybase-demo 'SELECT * FROM items WHERE id=7' --explain
```

CLI SQL accepts managed directories only; legacy files are rejected. The optional
parameters array is bounded typed JSON, never logged or inserted into SQL text.
Results are printed only after execution succeeds. Existing local database paths
remain trusted operator input; network authorization is a future server boundary.

## Bounds and unsupported behavior

Input: 16384 UTF-8 bytes, 4096 tokens, 64 statements. Expressions: at most 32
levels, including combined flat AND/OR trees. Projections, ORDER terms, assignments,
named insert columns and row values: at most 64 each. INSERT: at most 256 tuples.
Decoded quoted text: at most 3072 bytes; hex-literal content has the same bound,
so an inline byte literal currently carries at most 1536 bytes. Identifiers and
schema validation impose additional catalog limits. SQL errors report an offset
and generic expected category, without SQL, literals or parameter contents.

Execution: 100000 combined scan/join-candidate and predicate-node visits per script;
10000 intermediate rows per SELECT. Estimated retained intermediate row bytes are
capped at 8 MiB per SELECT; returned rows across the script share another 8 MiB cap.
This is a row-memory estimate, not a JSON/wire-byte limit. Full input scans still
use bounded existing table snapshots. Sorting is separately bounded by row/column
limits. ORDER BY can hit intermediate limits despite a small final LIMIT; without
sorting, selection stops at LIMIT. Writes share the existing 256-event/256-page
normal transaction limit; overflow rolls back the entire script.

Arithmetic, functions, aggregates, DISTINCT, GROUP BY, subqueries, RETURNING,
OFFSET, UNION, outer/cross joins, secondary-index DDL, ALTER, implicit casts and PostgreSQL
protocols are unsupported. Persistent indexes and stable schema migrations need
separate format/transaction decisions. Syntax/AST APIs are experimental.
