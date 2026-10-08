# Bounded logical table exchange

The synchronous emilybase-transfer crate and CLI exchange one complete small
public table using the original engine. This is editable logical data, not a WAL
backup or a PostgreSQL/Supabase converter. No existing database engine is used.
Use disposable synthetic data. Account-root backup/restore remains the separate
operation for private accounts, session scope and project credentials.

The UTF-8 JSON document has exactly format, version, schema and rows fields:

```json
{"format":"emilybase-table","version":1,"schema":{"name":"t","columns":[{"name":"id","data_type":"integer","nullable":false},{"name":"n","data_type":"float","nullable":false}],"primary_key":0},"rows":[[{"type":"integer","value":1},{"type":"float_bits","value":"8000000000000000"}]]}
```

Schema uses the existing bounded catalog contract. Values use null, boolean,
integer, text and bytes tags as in the catalog; bytes are arrays of integers0..255.
For float columns, use float_bits with exactly16 lowercase hexadecimal characters
encoding the IEEE754 binary64 bit pattern. This preserves finite values, negative
zero and subnormal bits without relying on decimal JSON conversion. Infinity and
NaN refuse. This value format is distinct from the HTTP query parameter contract.

Version1 limits input/output to8 MiB,255 rows,64 columns,63-byte identifiers and
3072-byte text/byte values, with the existing4000-byte encoded-record limits as
well. Array visitors reject excess elements before retaining them; bounded text
is checked before creating its owned value. The input byte cap also bounds JSON
scratch input, but is not a whole-process heap quota. Unknown/duplicate fields,
trailing nonwhitespace, unsupported versions, bad schemas/types and encoded-record
overflow refuse. Primary keys must be strictly ascending in the original integer
or UTF-8 byte order; duplicates and descending rows refuse before database opening.
No checksum authenticates editable logical input. Valid deliberate changes are
imported as new data, not detected as archive corruption.

Export borrows checked primary rows from an immutable owned database view, validates
the complete selected table, and constructs the bounded result before writing to
stdout. A table over255 rows refuses without silently truncating it. Source WAL is
unchanged. Import fully validates/owns input before opening an existing managed
database. It creates the new table and all rows in one ordinary WAL transaction:
one create plus at most255 inserts fits the existing256-event bound. An existing
table refuses, including an empty matching table. There is no merge, overwrite,
append, implicit database creation or partial success. Empty documents can create
one empty valid table. Existing engine page/WAL/state limits still apply.

## CLI

Initialize two disposable managed databases, create your synthetic source table,
then use a pipe. Values never need to enter command arguments:

```sh
cargo run --locked -p emilybase-cli -- db-init synthetic-source --durable
cargo run --locked -p emilybase-cli -- db-init synthetic-copy --durable
cargo run --locked -p emilybase-cli -- sql synthetic-source \
  "CREATE TABLE t(id INT PRIMARY KEY,title TEXT);INSERT INTO t VALUES(1,'synthetic')"
cargo run --locked -p emilybase-cli -- table-export synthetic-source t |
  cargo run --locked -p emilybase-cli -- table-import synthetic-copy
```

Import reads until EOF with an8 MiB+1 physical read cap. On success stdout contains
only transfer version/row/column/byte counts and the durable transaction ID. Stream
errors are returned; they do not trigger panic. A successful commit may still have
no observed output if stdout fails or the process stops afterward. Inspect the
existing table before retrying; repeated import refuses rather than duplicating it.
The caller owns pipe/file lifecycle. These commands do not atomically publish a
filesystem export, fsync redirected output or encrypt its contents. For private
operator files, set an appropriate umask/noclobber outside the checkout and inspect
partial output on failure. Never commit table documents containing real data.

Version1 is an experimental separate logical format; stored database/WAL/catalog
formats are unchanged. New incompatible logical documents must use a new version.
Large paginated transfers, foreign schema mapping, public authorization and stable
format/production acceptance remain open. See [ADR0089](adr/0089-bounded-logical-table-transfer.md).


## Project HTTP transport

Both server data modes expose POST /v1/projects/{id}/tables/export with strict
JSON {"table":"t"}, and POST /v1/projects/{id}/tables/import with the complete
logical document itself. Supply the project's service key through the existing
authorization header. Master, user access and refresh tokens cannot authorize
these routes. Route scope selects the database; body fields cannot select paths
or other projects. Public signup, user SQL authority and row policies remain open.

HTTP caps both request bodies and export output at65,536 bytes, retains the
five-second body deadline/four-worker admission and returns an error instead of
truncating a large table. The CLI retains its separate8 MiB limit. Both success
and error responses set no-store/no-cache. Export returns the typed document;
import returns {"transfer":{"version":1,"rows":1,"columns":2,"bytes":300},
"transaction":2}, with actual counts and transaction ID. The example byte count
is illustrative, not an encoded fixture. Preserve integer precision in clients.

Invalid documents, existing tables or excessive output return400 transfer_rejected;
body overflow413, deadline408 and wrong content type415 retain existing codes.
Storage/uncertain commit failures return503 and must be inspected before retry.
Private-root mode rechecks the current service key after body waiting; legacy mode
retains its already admitted capability semantics, as for SQL. Transfers do not
change private session clocks or account WAL. All decode/recovery/row/commit work
runs in bounded blocking tasks. Total heap/output-connection admission remains
separate work. See [OpenAPI](openapi.json) and
[ADR0090](adr/0090-project-scoped-http-table-transfer.md).
