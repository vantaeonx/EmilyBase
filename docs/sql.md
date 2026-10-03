# Experimental SQL subset

The original `query` crate currently provides a lexer, typed AST and parser.
It does not yet execute statements or expose a SQL CLI. Parser acceptance alone
does not guarantee that schemas, names or row values are valid in a database.
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
This parser does not implement binding/execution yet.

Predicates support column/literal/parameter operands, `=`, `<>`/`!=`, `<`, `<=`,
`>`, `>=`, `IS [NOT] NULL`, boolean operands, parentheses, `NOT`, `AND`, `OR`.
Precedence is NOT, then AND, then OR. UPDATE assignments and INSERT values accept
literals/parameters only. LIMIT accepts an integer 0..10000 or a parameter whose
value will need executor validation. ORDER BY uses source columns and optional
ASC/DESC, NULLS FIRST/LAST; sorting/null semantics follow in the executor.

Statements require separating semicolons; the final semicolon is optional.
Empty statements/scripts are rejected. Line `--` and non-nested `/* */` comments
are accepted outside quoted strings. Comments have no runtime behavior.

## Bounds and unsupported behavior

Input: 16384 UTF-8 bytes, 4096 tokens, 64 statements. Expressions: at most 32
levels, including combined flat AND/OR trees. Projections, ORDER terms, assignments,
named insert columns and row values: at most 64 each. INSERT: at most 256 tuples.
Decoded quoted text: at most 3072 bytes; hex-literal content has the same bound,
so an inline byte literal currently carries at most 1536 bytes. Identifiers and
schema validation impose additional catalog limits. SQL errors report an offset
and generic expected category, without SQL, literals or parameter contents.

Arithmetic, functions, aggregates, DISTINCT, GROUP BY, subqueries, RETURNING,
OFFSET, UNION, outer/cross joins, indexes, ALTER, implicit casts and PostgreSQL
protocols are unsupported. Persistent indexes and stable schema migrations need
separate format/transaction decisions. Syntax/AST APIs are experimental.
