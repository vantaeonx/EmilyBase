# ADR0100: Bounded schema-bound row decisions for borrowed private principals

Status: accepted as a synchronous library foundation; user data routes stay closed.

The existing trusted project service key grants broad project authority. Before
any end-user data access, establish an independent typed row decision boundary in
Rust. Accept only an actual current borrowed SessionPrincipal from the private
store; do not accept caller-supplied user IDs, role strings or a serialized proof.
The pure library returns a decision for exact rows, never a database handle or
reusable storage capability. HTTP/policy persistence and end-user RLS remain open.

Compile explicit version1 rules for SELECT, INSERT, UPDATE old/new and DELETE.
No missing/default allow. Deny and authenticated-all are deliberate empty-struct
variants so unknown JSON fields cannot be ignored by unit-variant deserialization.
Typed ownership, equality, nullable IS NULL and nonempty ALL/ANY use resolved column
indices; no SQL fragments, expression eval, subqueries, NOT or implicit coercions.
Owner means a16-byte bytes field matching the verified account ID in constant time;
NULL/malformed owner lengths deny. Equality is exact typed Rust value equality;
NULL uses an explicit predicate, finite float signed zero follows numeric equality.

Bind the compiled model to a canonical project, positive stable table ID and the
complete validated/physically encodable schema. Every evaluation rechecks trusted
current context and principal scope, validates complete typed/encodable rows, and
requires both UPDATE using(old) and check(new). Unsupported primary-key changes
refuse. Table recreation or any schema change requires a new binding. The caller
must source context/rows from and enforce the decision inside the authoritative
original transaction; this pure library does not enforce that integration for it.

Bound input to16384 bytes before Serde. Compilation shares64 nodes, depth8 and8192
literal payload bytes across all five rules, checks before cloning owned literals,
and resolves every branch even when a prior branch could allow. Models/errors
redact literals/identity. Decode/compile/evaluate are synchronous and perform no
I/O, KDF, private-clock observation or WAL writes. No stored engine codec changes.

Acceptance includes strict malformed/duplicate/unknown fields, aggregate bounds,
real borrowed proof scope and old/new owner checks, independent generated decisions,
exact private-history preservation and original transaction rollback/reopen plus
actual table recreation on both WALs. A dedicated sanitizer fuzz target covers the
strict document/compiler. Policy catalog/install/revision durability, role grants,
SQL/query filtering, atomic user CRUD and authenticated end-user HTTP must be
implemented/tested separately before claiming RLS or changing existing authority.
