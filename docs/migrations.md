# Bounded offline SQL migrations

`crates/migrations` applies explicit sequential SQL scripts to a managed project
**data** database through the original synchronous engine. No external database,
new engine event, WAL version or file codec is introduced. Do not point this tool
at the separate private account/session database. Server SQL/service keys remain
their own authority; there is no migration HTTP endpoint or dashboard yet.

```sh
emilybase db-init ./synthetic-data --durable
emilybase migrate ./synthetic-data 1 initial <<'SQL'
CREATE TABLE notes(id INT PRIMARY KEY,title TEXT);
INSERT INTO notes VALUES(1,'synthetic');
SQL
emilybase migrations ./synthetic-data
emilybase migrate ./synthetic-data 2 add-labels <<'SQL'
CREATE TABLE labels(id INT PRIMARY KEY,title TEXT);
SQL
```

SQL arrives on stdin, capped at16,384 UTF-8 bytes before opening/recovering the
selected database. Output is JSON receipt metadata, without SQL/row contents.
Errors do not echo the input script. Destination ownership is exclusive; stop
its server first. There is no implicit creation, reset, repair, lock retry or
version skipping. Existing typed/SQL/backup commands preserve their contracts.

Versions start at1 and continue through at most128. A label is1..63 ASCII bytes,
starts with a letter/digit and contains letters/digits/underscore/hyphen. Scripts
use the currently implemented CREATE TABLE, DROP TABLE, INSERT, UPDATE and DELETE
subset; SELECT and every SQL transaction control are rejected. Parameters have no
binding array in this API, so unresolved parameters refuse execution atomically.
Parsed statement targets cannot address the reserved ledger (case-insensitive
refusal); ledger-looking text literals remain data. Existing SQL token, statement,
query-work, physical record, page and WAL bounds still apply.

`prepare` validates/hash-binds immutable borrowed input without a destination.
`apply` validates the whole existing ledger before checking order or exact retry.
An identical already applied version returns its original receipt/transaction and
`already_applied: true`, even after later migrations, without a WAL write. Changed
label or any changed SQL byte (including whitespace/comments) refuses. A new version
must be exactly the next one. Apply stages optional ledger creation, SQL and the
receipt in one transaction and acknowledges only after the original WAL commit
syncs. Late execution or receipt-capacity failure discards everything. An ambiguous
write result is propagated; reopen and check the same definition before retrying.
No automatic retry can duplicate an unacknowledged schema/data change.

## Receipt layout and compatibility

The conventional `_emilybase_migrations_v1` table has exactly four non-null columns,
with integer `version` as its primary key:

| Column | Original catalog type | Validation |
| --- | --- | --- |
| version | integer | consecutive1..N, N≤128 |
| label | text | label rule above |
| sha256 | bytes | exactly32 bytes |
| transaction | text | canonical positive u64 decimal, ≥2, strictly increasing, ≤current commit |

Digest input is the concatenation of literal bytes `emilybase-migration-v1` plus
NUL, version as big-endian u32, label byte length as big-endian u32, exact label
bytes, SQL byte length as big-endian u32, and exact SQL bytes. No trimming, newline
normalization, Unicode normalization or SQL rewriting occurs. A separate hashlib
vector freezes this construction. SHA-256 is an identity/integrity check against
the supplied definition, not a signature or an authenticity proof.

A missing ledger means no recorded migrations. An existing empty/wrong-schema,
overfull, gapped or malformed ledger refuses inspection/application; it is never
silently reset. Receipt transaction IDs are assigned under the exclusive database
owner, in the same commit; root initialization occupies transaction1. Ordinary
writes between migrations are permitted, so receipt commit numbers can have gaps.
The ledger uses normal typed rows and is included in verified backups, restore,
replay and compaction. Existing databases without it remain readable and can apply
version1. Future receipt versions require an explicit compatibility decision.

The ledger consumes one table and one row per migration from ordinary engine
limits. The first migration leaves at most254 event slots for its script (ledger
create+receipt consume two); later ones leave255. A script creating a table itself
also spends an event. Thus first-migration CREATE plus253 inserted rows fits256
exactly; one additional row refuses the entire migration. Table/page/WAL bounds
may refuse earlier. These limits cannot be bypassed by splitting SQL statements.

## Remaining boundaries

The ledger is conventional metadata readable/writable by the trusted database
owner/service SQL authority. Such an owner can delete or forge it; structural
checks cannot reconstruct deleted history or authenticate forged32-byte digests.
Do not treat receipts as a permission system or independent audit log. Hashes/labels
are metadata and may reveal information about sensitive scripts; keep secrets
out of migration definitions and use synthetic examples at this stage.

No automatic schema diff, ALTER, down migrations, script directory discovery,
remote migration runner, large multi-transaction migration, online coordination,
PostgreSQL compatibility or production readiness is claimed. Controlled child kills
verify staged/ACK recovery on WAL1/2; full power-loss/upgrade/security/resource
acceptance remains open.
