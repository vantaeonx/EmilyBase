# ADR 0037: standalone typed page namespaces and root bindings

Status: accepted for experimental codec validation only. Durable index/WAL
integration in ADR 0031 remains proposed, and no new runtime format is enabled.

## Decision

Add a synchronous `commit-format` crate with fixed-size EBNS-1 address and EBIR-1
root records, typed constructors/errors, exact-length decoding and bounded fields.
Keep it outside the runtime storage/WAL dependency path. CRC detects corruption;
it is neither authorization nor proof of selected page/row correctness.

Current relational pages contain append-only events from multiple tables. Their
namespace is database-global (`RelationalHistory`, table scope zero). Primary-index
pages use a nonzero persistent table ID. Identity is database/domain/table/page;
equal page IDs in different tables or domains do not alias. A table ID is not a
current table count and is not limited to 128: table creation/drop can advance it.

Root records contain the primary page namespace, integer/text key type, local tree
revision, owning database transaction, counts and an optional exact predecessor.
The first revision is one with no predecessor. A later revision requires exactly
the preceding tree revision and a strictly older owning transaction; skipped
global transactions are valid when that table's tree did not change.

Predecessor fingerprint bytes describe an exact canonical index state, supplied
and verified by the caller. A noninitial fingerprint may contain all zero bytes;
zero is not a forbidden SHA-256 value. The absent predecessor is encoded with all
its fields zero. Root changes may select another arena page, but cannot change
database/table/key type or bypass the exact base. Reused arena addresses alone
do not select a newer state. Unknown versions/domains/types and nonzero reserved
fields fail closed, including records with recomputed CRCs.

Address bounds follow the existing 65536 history-page and 1024 primary-page limits.
Roots admit 1..1024 materialized pages and at most 10000 covered plus excluded keys.
Integer roots exclude no keys; text roots account for the existing long-key path.
Sparse stable IDs are not forced below the live page count. Counts do not establish
topology, key coverage or current row-image correctness. Those checks belong to
the next independent state model and eventual complete-state validation.

## Compatibility and limits

The layouts are documented in [experimental commit metadata](../commit-metadata-format.md).
They are standalone records, not a WAL-3 definition or a migration. Old WAL 1/2,
EMILYDB/EBPG/EBIX/EBIF/ETBL/EBTI and backups keep their bytes and behavior. Runtime
readers still reject unknown WAL versions. No mandatory second index file is added.
No database, table data or index image is written by this codec.

Before runtime enablement, specify combined page/byte/memory budgets and prove
one table/index commit decision, validated roots/rows, retirement, recovery and
verified migration. Standalone checks close no broader power-loss/security gate.
