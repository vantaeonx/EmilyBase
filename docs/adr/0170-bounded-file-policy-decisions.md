# ADR0170: bounded file-policy decisions over borrowed private principals

Status: accepted for experimental synchronous decision-library use.

## Decision

Implement a small, flat, versioned file-policy language in auth, independently of
the native file engine. Require all four actions explicitly and cap input at1024
bytes. Existing-reference rules are deny, owner and authenticated; creation permits
deny or self_owned only. No rule inherits permission from another operation.

Bind the compiled model to a canonical project and exact native database ID.
Require a genuine borrowed SessionPrincipal from the private account store and
compare every owner byte using timing-safe equality. The trusted adapter supplies
actual selected native context and current reference metadata; creation derives
its proposed owner from that principal. Scope admission precedes metadata lookup,
including empty or missing results.

Keep compilation and evaluation synchronous and free of storage, time and catalog
observations. Expose typed redacted errors and no reusable permission receipt.
The principal borrow excludes concurrent credential mutation in its original
store. A caller must obtain it at current trusted time and consume it immediately;
the model cannot independently extend token expiry or establish installed policy.

## Acceptance and remaining work

Exercise strict syntax and exact bounds, the complete54-rule matrix under both
WALs, generated owner-byte decisions, current/foreign principals, each context byte,
credential revocation/rotation/restore, real native owner metadata and a bounded
parser sanitizer campaign. See the [contract](../file-policy-decisions.md).

An installed private catalog and current-policy proof, actual owned native user
operations, HTTP/SDK adapters, roles and signed URLs require separate changes.
This decision changes no persisted schema, database/WAL format or service-key API.
