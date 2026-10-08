# Original policy record group v1

Pure experimental `row_policy::records` codec for a future private policy catalog.
Current private account/session stores remain v1..v3; their accepted schemas and
root initialization are unchanged. Do not insert these tables into an existing
private store manually: current complete-schema validation will refuse it.
There is no policy-install command or HTTP route yet.

`encode(TableContext, revision, previous, document)` validates the strict bounded
policy against its exact target schema and returns normal typed rows. It does
not write them. `header_schema()`/`chunk_schema()` describe exact proposed private
table shapes. `inspect(expected_project, header, chunks)` verifies a complete
ordered group, then returns exact document metadata and a bound decision model.
It grants no original database handle or user authority.

| Header field | Type and constraint |
| --- | --- |
| table | canonical decimal-string u64, positive, primary key |
| version | integer1 |
| project | canonical32 lowercase hex, must match trusted expected scope |
| revision | canonical decimal-string u64, minimum2 |
| previous | canonical0 or u64 minimum2, strictly below revision |
| schema_length | integer1..4000 |
| document_length | integer1..16384 |
| sha256 | exactly32 bytes |

Payload is exact original encoded Schema bytes followed by exact policy document
bytes. Split at3072 bytes. Each chunk is `[Text("table:index"), Bytes(payload)]`,
with canonical decimal table/index and zero-based consecutive indices. All except
the last fragment have exactly3072 bytes; the last has exactly the remaining
positive size. Maximum body20,384 bytes gives at most seven chunks plus one header.
Each independently fits a normal4000-byte record. No page-size/engine limit changes.

Checksum input, in order: ASCII domain `emilybase-row-policy-records-v1` plus NUL;
32 ASCII project bytes; big-endian u64 table/revision/previous; big-endian u32
schema/document lengths; complete schema+document body. Canonical metadata refuses
before recomputing the digest. Digest comparison is constant-time. Exact definition
whitespace is preserved and changes the digest. Floats/literals follow the strict
[decision-model contract](row-policies.md), not SQL text or executable expressions.

Inspection refuses extra/missing/reordered/duplicate/missized fragments, unexpected
keys, malformed canonical integers, length overflow, unsupported version or checksum
mismatch. Bounds precede combined allocation. After integrity, it decodes original
schema bytes and compiles every policy branch, retaining full type/node/depth/literal
checks. Even a repaired checksum does not make malformed nested data valid. The
checksum is not a signature: a trusted storage owner can replace a valid policy.
Documents are plaintext record contents; Debug/errors redact them.

A future catalog writer must allocate revisions under its exclusive original owner,
check expected revision/context, commit header and all fragment changes atomically,
validate complete inventories/orphans, preserve revocation semantics and include
policies in root capture/restore. The codec itself cannot enforce those caller
steps. Current tests compose ordinary typed transactions and verified backup/restore;
they do not claim an installed policy subsystem or new runtime RLS authority.
This is an experimental logical record version, not a stable production file format.
