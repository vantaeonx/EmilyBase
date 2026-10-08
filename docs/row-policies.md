# Bounded row policy decision foundation

`emilybase_auth::row_policy` is a synchronous pure Rust library. It accepts a
current borrowed private `SessionPrincipal` and exact typed rows. It supplies no
HTTP route, role grant or storage handle. Persistence is supplied separately by
the explicitly enabled [private v4 catalog](policy-catalog.md). End-user tokens
still cannot call project SQL/rows/migrations. Existing trusted service authority
is unchanged; row-level security is not enabled by this increment.

A complete version1 definition has five explicit rules. Example:

```json
{
  "version": 1,
  "select": {
    "kind": "any",
    "terms": [
      { "kind": "owner", "column": "owner" },
      { "kind": "equal", "column": "visible", "value": { "type": "boolean", "value": true } }
    ]
  },
  "insert": { "kind": "owner", "column": "owner" },
  "update_using": { "kind": "owner", "column": "owner" },
  "update_check": { "kind": "owner", "column": "owner" },
  "delete": { "kind": "owner", "column": "owner" }
}
```

This allows an authenticated verified user to read its own rows or visible rows,
insert/delete its own rows and update a row only when both old and new owners
match. It grants no anonymous read. SELECT is independent of the other operations;
there is no implicit SELECT requirement added to UPDATE/DELETE. Owners are bytes
fields with exactly16 bytes equal to the verified account ID, not an arbitrary
login or request parameter. NULL/short/long owners do not match.

Other rules are `{ "kind": "deny" }`, deliberate
`{ "kind": "authenticated" }`, `{ "kind": "is_null", "column": "note" }`
and nonempty `{ "kind": "all", "terms": [...] }`. A deny-only definition denies
all operations. Missing/duplicate/unknown fields and unknown kinds refuse; empty
ALL/ANY refuse rather than acquiring identity-based allow behavior. Every branch
resolves before evaluation, even in an ANY containing authenticated-all.

Equality literals use the original catalog tagged Value JSON, with exact declared
types, finite floats and3072-byte text/bytes bounds; NULL equality is rejected in
favor of explicit nullable-column IS NULL. No casts, SQL text, code execution,
regex, NOT, functions, subqueries or dynamic column names are evaluated. Text and
bytes compare exactly; float+0 and-0 compare numerically equal. Debug/error strings
contain no literals, account IDs or project names.

Decode caps documents at16,384 bytes before Serde. Compilation caps total nodes
at64, depth at8 and combined literal payload at8192 bytes across all five rules.
Scalar literals charge8 bytes each; text/bytes charge their payload length. This
is not a quota for total heap, schema metadata or whole-process memory.
The schema must satisfy original catalog/physical encoding bounds. Compiled
models own bounded literals/schema and bind a canonical project, stable table ID
and complete schema. A trusted caller supplies current `TableContext`; it must
match on every evaluation. A dropped/recreated table has a fresh ID and refuses an
old binding even when its name/schema is identical. Schema changes also refuse.

`BoundPolicy::authorize` checks the actual borrowed principal project and exact
context before inspecting rows. It validates complete row types and original
physical encoding even for authenticated-all; UPDATE checks both rows and refuses
a primary-key change. It makes no private time observation, database access or
write. The proof cannot be constructed from user metadata or serialized as a
capability; existing private verification/revocation/restore semantics apply.

The caller must keep authoritative transaction/context/row ownership through the
decision and write. Returning a decision does not enforce unrelated SQL. The
[owned root gateway](user-row-enforcement.md) now performs current installed-policy
verification and atomic typed CRUD under both original owners. Role membership,
bounded filtered ordering/continuation and end-user HTTP admission remain required
before enabling user data routes. No production or security-audit gate is closed.


The separate [bounded original record codec](policy-records.md) now packages
exact schema/document bytes into one header plus at most seven normal typed
fragments. It verifies complete integrity and recompiles nested policy constraints.
The explicit [v4 catalog](policy-catalog.md) supplies atomic installation/revisions
and current borrowed policy proofs. The root gateway applies these proofs to
exact-key reads and atomic typed writes; it exposes no end-user HTTP route.
