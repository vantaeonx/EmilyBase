# Single-row file-policy records

auth::file_policy::records encodes and inspects complete bounded policy data using
the original typed row codec. It specifies a future private catalog representation.
It performs no filesystem work, schema migration, transaction allocation or policy
installation. Native backend file operations continue to require current service keys.

## Exact typed schema and compatibility

The proposed auth_file_policies schema has primary key column0. All columns are
non-null, with exact order and types:

| Column | Type | Constraint |
| --- | --- | --- |
| database_key | Text | Exact32-character lowercase hex of the native database ID |
| version | Integer | Record version1 |
| project | Text | Canonical32-character project from trusted context |
| revision | Text | Canonical unsigned decimal u64, at least2 |
| previous | Text | Zero or at least2, strictly below revision |
| database | Bytes | Exact16 native database bytes, including opaque zero IDs |
| document | Bytes | Complete original supported policy, at most1024 bytes |
| sha256 | Bytes | Exact32-byte domain-separated checksum |

The row uses existing EROW version1; there is no new database, WAL or page version.
Record version and document version are separate checks. No normalization or
fragmentation occurs. The codec rejects extra/missing cells, wrong types, invalid
versions/lengths, noncanonical numbers/keys, scope mismatch, corruption and invalid
policy grammar. Decode of the outer binary row retains its existing4000-byte cap.
All54 rule definitions padded to1024 bytes roundtrip through a real4096-byte Page.

Expected project and database come from actual selected native context. Stored
scope cannot establish its own expected authority. The digest detects corruption;
a privileged writer can recompute it. A valid record alone grants no current
installed policy, session or file access. Revision numbers supplied to encode are
data; a future trusted catalog must allocate the actual commit and validate that
stored revisions do not exceed confirmed history.

## Digest and exact byte identity

SHA256 covers these concatenated values in order:

1. ASCII domain emilybase-file-policy-record-v1 and one NUL byte.
2. Record version as an unsigned16-bit big-endian integer.
3. Canonical project ASCII bytes and exact16 native database bytes.
4. Revision and previous as unsigned64-bit big-endian integers.
5. Original document length as unsigned32-bit big-endian, then exact document bytes.

Every fixed-width field is unambiguous and document length is bounded before hashing.
Checksum comparison uses the existing timing-safe library. Whitespace changes the
checksum even when the parsed definition is equivalent. The independent synthetic
fixture uses project11111111111111111111111111111111, database[3;16], revision9,
previous5 and the compact deny-all definition in declaration order. Its domain
digest is5797c7d15aaf9a4d2b3a9854e8e4f53e65411e7b68bf1c3d6d6d3300e577b6e1;
the263-byte EROW has SHA256c98146ea91e91d59ff314779aad2f912dc5b6205ceb887d792af1801c067be86.
Separate Python struct/hash calculations and Rust tests agree on both values.

## Privacy and model boundary

PolicyRecord wraps the encoded row with redacted Debug; raw row borrowing/moving
is explicit for trusted storage code. Raw Value rows, document bytes and metadata
must not be logged or serialized into public responses. DecodedPolicy also uses
redacted Debug and preserves exact original bytes for trusted retry comparison.
Its check_context/authorize methods delegate to the
[borrowed-principal decision model](file-policy-decisions.md). Compilation still
does not prove current installation. Genuine private identity and trusted current
context remain mandatory; no fabricated principal is used by parser fuzzing.

The first encoder returned an ordinary Row. A new privacy regression test failed
because its Debug included project scope. The encoded wrapper makes that same
test pass while preserving the independent wire vectors. No real secrets were used.

The source-bound [verification report](measurements/2026-10-20-file-policy-records/verification.json)
records actual stable/minimum checks, generated cases, independent vectors and
bounded ASan evidence. Installed private catalog, explicit migration, actual user
file enforcement, HTTP/SDK operations, roles and production acceptance remain open.
