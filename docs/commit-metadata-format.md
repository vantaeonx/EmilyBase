# Experimental commit metadata, version 1

These standalone `emilybase-commit-format` codecs are experimental and do not
change the formats read/written by any running managed database. There is no
new WAL selection or migration. See [ADR 0037](adr/0037-experimental-commit-namespaces.md).

All multibyte integers are unsigned little endian. Decoding requires the exact
record length; shorter inputs and trailing bytes are errors. Reserved bytes must
be zero. CRC32 uses the existing IEEE implementation over all bytes preceding the
last four-byte checksum. Repaired CRCs do not bypass field/namespace validation.
CRC and a decoded root do not authorize a project or verify page/row content.

## EBNS-1 address: 64 bytes

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 8 | `EBNS` followed by four zero bytes |
| 8 | 2 | Version, exactly 1 |
| 10 | 1 | Domain: 1 relational history, 2 primary index |
| 11 | 1 | Reserved zero |
| 12 | 16 | Nonzero database identity |
| 28 | 8 | Table scope |
| 36 | 8 | Page ID |
| 44 | 16 | Reserved zero |
| 60 | 4 | CRC32 of bytes 0..60 |

History is global to a database, so scope is zero and page IDs are 1..65536.
Primary-index scope is a nonzero table ID, with page IDs 1..1024. Table IDs can
exceed the current live-table count. Different domains/tables/databases retain
different addresses even with equal page IDs. An address carries no tree revision.

## EBIR-1 root: 192 bytes

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| 0 | 8 | `EBIR` followed by four zero bytes |
| 8 | 2 | Version, exactly 1 |
| 10 | 1 | Key type: 1 integer, 2 text |
| 11 | 1 | Reserved zero |
| 12 | 16 | Nonzero database identity |
| 28 | 8 | Nonzero table ID; domain is implicitly primary index |
| 36 | 8 | Selected root page ID, 1..1024 |
| 44 | 8 | Tree revision, nonzero |
| 52 | 8 | Owning database transaction, 1..2^64−2 |
| 60 | 8 | Predecessor tree revision |
| 68 | 8 | Predecessor owning transaction |
| 76 | 32 | Exact predecessor state fingerprint |
| 108 | 8 | Keys covered by the primary tree |
| 116 | 8 | Keys excluded to the existing long-text path |
| 124 | 4 | Selected materialized page count, 1..1024 |
| 128 | 60 | Reserved zero |
| 188 | 4 | CRC32 of bytes 0..188 |

Covered plus excluded counts cannot overflow or exceed 10000. Integer roots have
zero excluded keys. The sparse arena root ID need not be below the page count;
complete topology and coverage need separate validation. A syntactically valid
record can still describe an incorrect or foreign state and must not be published
without ownership, exact-base, topology and current-row checks.

Revision one has no predecessor: bytes 60..108 are all zero. Later revisions
require predecessor revision plus one equal to current revision, without overflow;
the predecessor owning transaction is nonzero and strictly older. The last value
2^64−1 is reserved for transaction-counter exhaustion. Tree and global transaction
numbers are distinct. Predecessor fingerprint bytes can be zero when a predecessor
exists; only the absent predecessor requires all fields zero.

`verify_owner` compares database/table/transaction. `verify_predecessor` compares
database/table/key type and the caller's exact previous revision, transaction and
canonical state fingerprint. It permits the selected root page to move. It does
not compute fingerprints, check a secret, inspect pages or recover a transaction.

## Compatibility policy

Unknown versions, domains/types and reserved bits are rejected. New incompatible
layouts need a new explicit version and specification; existing bytes must never
be silently reinterpreted. These limits are prototype admission, not a permanent
production-format promise. Runtime WAL integration remains independently gated.
