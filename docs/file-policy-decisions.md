# Bounded file-policy decision library

The synchronous auth::file_policy module decodes, compiles and evaluates explicit
rules for read, create, rename and remove. This is the decision component for
future user file operations. The current native file gateways still require a
project service key. No user-facing file route is installed by this component.

## Document and compatibility

Version1 JSON is at most1024 bytes including whitespace. Every field is required;
unknown fields, duplicate keys, missing actions, unsupported rule kinds, invalid
UTF-8 and trailing input fail with a redacted typed error. Decode validates syntax;
compile separately refuses unsupported versions. There is no permissive default.
Definition::deny_all explicitly constructs four closed rules.

```json
{
  "version": 1,
  "read": {"kind": "owner"},
  "create": {"kind": "self_owned"},
  "rename": {"kind": "owner"},
  "remove": {"kind": "deny"}
}
```

| Action | Accepted rule | Decision after scope admission |
| --- | --- | --- |
| read, rename, remove | deny | Refuse |
| read, rename, remove | owner | Require exact16-byte account/owner equality |
| read, rename, remove | authenticated | Allow the verified current project principal |
| create | deny | Refuse |
| create | self_owned | Require the proposed owner to equal that principal |

Each action is independent. Read permission supplies no rename/remove permission;
rename may be permitted while reading is denied. Creation cannot assign another
account as owner. Listing, quotas, administration, paths, roles, anonymous access
and signing are outside this grammar. It contains no expression or SQL evaluation.

## Trusted inputs and original authority

BoundPolicy::compile binds a canonical project and exact native database ID.
Database IDs remain opaque16-byte values. StoreContext must come from the actual
selected file component. Change owners must come from authoritative current native
reference metadata; a future create adapter must derive ownership from the session.
Neither context nor owner assertions from an untrusted request establish authority.

check_context admits the exact model/context/principal project and database before
any reference lookup, including a missing reference or empty result. authorize
repeats this check and evaluates one action. Both require SessionPrincipal, whose
private constructor belongs to AccountStore::verify_access. Owner comparisons use
the existing timing-safe library across all16 bytes. Errors, definition/model Debug
output and rule Debug output omit account IDs, policy details and credentials.

The principal borrows its actual private store, preventing mutation of that store's
credentials while borrowed. The decision library reads no clock or filesystem.
Its caller must verify the current token at trusted current time and consume the
decision immediately within the real original storage operation. Holding a proof
does not extend expiry, make another store current or protect a changed namespace.
BoundPolicy is an immutable model with no cloned/serialized permission receipt.

A compiled arbitrary document is not proof of current installed policy. The next
integration requires an explicitly versioned private catalog, current installed
policy proof under the private owner, exact native binding and evaluation using
current reference metadata inside actual native checks. Existing service-key
mutations retain their separate backend authority and opaque owner metadata.

## Verification and release boundary

The new tests exercise strict grammar/bounds and all54 explicit rule combinations,
two owners and four operations under both WAL versions:864 independent decisions.
Generated cases cover arbitrary bytes, rule roundtrips and genuine-principal
decisions. Scope cases change each native database byte and use a foreign project.
Logout, account disabling/re-enabling, password change, refresh rotation, expiry
and verified private restore must refuse stale credentials before any principal.
A compile-fail example verifies that borrowed credentials cannot be changed.

The server case uses actual native reference owners and database metadata under
both WAL versions, verifies foreign-owner/project refusal, unchanged source
histories and current-key backend behavior. It evaluates the model through a real
private principal; it does not install a user-file storage authorization route.
The file_policy fuzz target decodes, compiles and checks normalized roundtrips
with no fabricated principals, filesystem access or credential hashing.

The source-bound [verification report](measurements/2026-10-20-file-policy/verification.json)
records executed checks and their limits. No persisted schema/format or recovery
rule changes. Policy installation, actual user-file enforcement, HTTP/SDK file
access, roles, signed URLs and production acceptance remain open.
