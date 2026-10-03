# Typed records and relational events v1

These records are payloads inside the existing v1 slotted pages. The page/file
version remains unchanged. The record and relational versions are independent;
unknown versions fail closed. Old raw-page files remain usable by raw commands.

## Catalog and row codec

All integers are little-endian. An encoded schema or row is bounded to 4000 bytes.
A variable-length blob is a u16 byte length followed by exactly that many bytes.
Text must be valid UTF-8. Values are bounded to 3072 bytes; rows to 64 columns.
Names are 1..63 ASCII bytes matching `[A-Za-z_][A-Za-z0-9_]*`, case-sensitive.

Schema: `ESCH`, u16 version 1, u16 primary-key column position, u16 column count,
table-name blob, then each column's name blob, u8 type and u8 nullability.
Types: 1 boolean, 2 signed i64, 3 finite IEEE-754 f64, 4 text, 5 opaque bytes.
Nullability accepts only 0 or 1. Column names are unique. The primary key is a
non-null integer or text column. Composite keys and collations are unsupported.

Row: `EROW`, u16 version 1, u16 value count, then tagged values. Tag 0 is null;
tag 1 has a boolean byte (0/1); tags 2/3 have eight bytes; tags 4/5 have a blob.
NaN, infinity, unknown tags, truncated or trailing bytes are rejected. No SQL
parser or PostgreSQL binary compatibility is implied by these types.

## Relational envelope

Every event has `ETBL`, u16 version 1, u8 kind, one zero reserved byte, u64 table
ID, followed by the payload. Envelope plus payload is at most 4058 bytes.

| Kind | Meaning | Payload |
| --- | --- | --- |
| 0 | initialized database marker | empty; table ID must be 0 |
| 1 | create table | schema |
| 2 | drop table | empty |
| 3 | insert row | row |
| 4 | replace row | row |
| 5 | delete row | one-value row containing an integer/text key |

All non-root events require a nonzero table ID. The table engine validates event
ordering, table existence and uniqueness when replaying the file. Root must be
the first record (page 1, slot 0) and cannot recur. History slots cannot be deleted
or empty. New table IDs are sequential and are never reused after drop. Table
names may be reused with a new ID. Insert requires a new key; replace/delete
require an existing key. Every row is checked against its current schema.
This is a table history in data pages, **not WAL**: rewriting its last page can
still tear on a crash. No transaction or recovery guarantee is introduced.

The live state is bounded to 128 tables and 10000 rows globally. History is bounded
to 100000 events including root. Scans are in primary-key order with an explicit
limit of at most 10000 rows. A failed validation writes no event. A storage write
failure poisons the handle; close it and inspect the file rather than retrying.

Golden byte fixtures are synthetic Rust test literals. Any incompatible codec
change must increment its version and provide an explicit converter that keeps
the original file. No silent conversion of raw-page files is permitted.
