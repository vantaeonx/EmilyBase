# Explicit private v4 policy catalog

`AccountStore::enable_row_policy_catalog()` explicitly migrates an already clocked
v3 private store to v4 in one original WAL commit. It creates the exact policy
header/chunk tables and changes `auth_scope.version` together. Opening, requests
and default root initialization do not perform this migration. v1/v2 first need
explicit existing clock activation. Users, verifiers, incarnation, session families
and clock remain unchanged. Identical migration retries write nothing.

v4 adds exactly two tables to the five v3 tables. It preserves the original
4096-byte page and WAL1/2 formats. Readers predating v4 refuse this private logical
schema; downgrading or removing its tables manually is unsupported. Keep a verified
pre-migration backup for an explicit offline rollback to the older snapshot.
No production format or rolling-upgrade compatibility is promised.

`install_row_policy(context, expected, document)` is a trusted synchronous library
operation. The caller obtains project, stable table ID and complete schema from
the held public database owner; client-supplied assertions are insufficient.
Definitions follow the [bounded decision contract](row-policies.md). Exact original
bytes and schema are stored through the [record-group codec](policy-records.md).

The expected revision is0 for a missing policy, otherwise the current receipt's
revision. Changed content with a stale expectation refuses. A current exact schema/
definition with either its current revision or recorded predecessor expectation
returns the existing receipt without a commit. This supports an explicit retry
of a lost result; callers must not automatically retry ambiguous writes. Revisions
are actual private committed transaction IDs and can have gaps from unrelated
session/user writes. Replacing a policy deletes all prior chunks and publishes the
complete new header/chunks in one original commit. Definition whitespace matters.

The catalog holds at most128 stable table identities, each with at most seven
3072-byte fragments and one header. Normal engine row/page/WAL limits also apply;
these count bounds are not whole-process memory reservations. An existing policy
can be replaced at capacity. There is no deletion/reuse operation: install deny
rules to disable access. Retirement of obsolete table identities needs a separate
tombstone/ABA design before any capacity-reclaim command is enabled.

`row_policy_receipts()` performs read-only complete validation and returns sorted
numeric table identities, revision, predecessor and digest. No policy document,
password verifier or token is returned. These Rust u64 values are not a JavaScript
wire contract; a future HTTP DTO must encode all digits safely.

`verify_row_policy_access(token, trusted_now, table)` validates the complete catalog,
loads the installed model and verifies the current access session under the same
exclusive private owner. It returns a non-cloneable, non-serializable borrowed
`PolicyPrincipal`. Its `authorize(context, change)` checks exact current table
scope/schema and typed rows. The owner cannot replace policies or credentials
while this proof is borrowed. A replacement affects the next verification without
requiring a new user token. Missing catalog/policy never grants row authority.
Existing durable time observations and session revocation rules still apply.

Complete open/export/inspection/root validation refuses unknown tables/schemas,
orphans, missing/extra fragments, invalid nested definitions, checksum damage,
noncanonical identities and a revision beyond the actual private committed LSN.
No read/list/export path silently repairs corruption. Backup and common-root restore
preserve policy groups and revisions; private restoration still installs a fresh
session incarnation and trusted clock before publication, revoking old tokens.
The root's service key and original public data identity remain preserved.

This is policy persistence and a current borrowed decision API. User SQL/data routes,
role membership, bounded filtering/ordering/continuation, public transaction
context and atomic user CRUD enforcement remain open. Project-service routes keep
their existing trusted authority. No end-user HTTP route or platform milestone is
enabled by this change. See [ADR0102](adr/0102-explicit-private-policy-catalog.md).


The retained root now derives the actual held table context for trusted installation.
Its [service-only HTTP administration](policy-administration.md) adds explicit
migration/list/install with exact revision strings. This does not enable user data
routes, role grants or runtime row filtering. Existing direct AccountStore callers
still bear the trusted-context obligation described above.
