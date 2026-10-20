# ADR0171: bounded file policies in one original typed row

Status: accepted for experimental pure record-codec use.

## Decision

Represent one complete file policy with the existing original catalog row codec.
The1024-byte document cap permits a single row within the existing slotted page;
no chunk table or additional binary envelope is required. Specify an exact private
schema with canonical database key, record version, project, revision/previous,
native database bytes, original document and a domain-separated SHA256 digest.

Inspect against a trusted selected project/native database, independently of stored
claims. Require exact arity/types, canonical full-width decimal revisions, original
document bytes, digest and supported grammar before creating an immutable decoded
model. Preserve whitespace and byte identity. Encoding allocates no transaction ID
and creates no on-disk table. A later catalog must commit the row with its actual
revision and validate confirmed history before supplying installed authority.

Return sensitive encoded data through PolicyRecord with redacted Debug and explicit
raw-row access. Keep decoded Debug redacted. A reproduced preliminary regression
showed ordinary Row Debug exposing project scope; the wrapper fixes that boundary
without changing the wire bytes or weakening the codec.

## Acceptance and limits

Use independent Python digest/binary vectors, all54 maximum-document definitions
through an actual page, full-width revisions and opaque IDs, byte corruption,
checksum-repaired semantic damage, generated rows and a bounded sanitizer run.
See the [record contract](../file-policy-records.md).

The schema is specified but not installed. No private schema migration, policy
administrator, current installed proof, user-file route, new database/WAL format
or production acceptance is supplied by this codec.
