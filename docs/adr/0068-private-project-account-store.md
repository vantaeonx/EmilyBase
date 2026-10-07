# ADR0068: private project account storage on the original engine

Status: accepted for the local library; network/registry integration pending.

## Decision and boundary

Add AccountStore to the synchronous auth library, backed by the existing
original-engine Database and mandatory WAL. Use a separate private directory,
not a table in a project's public SQL data directory. No finished database engine,
external identity provider or paid service is introduced. Existing server routes
do not create or discover account stores and cannot access this directory.

The trusted local caller selects the path and supplies the canonical 32-character
project ID. Store creation rejects invalid scope before I/O; opening checks the
persisted scope against the caller's expected project before returning an owner.
This is a scope consistency check, not authorization of an untrusted caller:
future server code must select paths from existing authorized project capabilities.
The original engine retains its private permissions, exclusive directory/WAL
ownership, no-clobber creation, typed uncertainty, commit/rollback and recovery.

## Private schema version 1

Exactly two live schemas are permitted. auth_scope has integer primary id,
integer version, text project and bytes dummy. Exactly one row has id=1/version=1,
canonical matching project ID and a strict EBPWD verifier. auth_users has text
primary login, 16-byte random identity, strict EBPWD verifier, positive i64
credential epoch and Boolean disabled. Both schemas have only non-null columns.
Unknown versions, additional tables, changed column meanings, duplicate identities
and semantically invalid records fail closed even with valid engine checksums.

Login names contain 1..64 lowercase ASCII bytes, start with a letter/digit and
then permit letters, digits, dot, underscore and hyphen. Uppercase, whitespace,
Unicode, controls and separators are rejected, never normalized. Password bytes
retain the separate Unicode/NUL/exact-byte contract. Display names/email handling
are future policy; login names never enter file paths or interpolated SQL.

Cap each private store at 1024 accounts, including disabled ones. Exact two-table
and one-scope-row invariants make the total snapshot count minus one the account
count. Opening consumes checked primary-row cursors, validates every record and
tracks unique 16-byte identities. Provisioning chooses OS-random identity with
at most four collision attempts. Bounded identity sets and indexes still allocate;
this account cap is not a numeric whole-memory quota.

This private schema is independent of page/WAL/archive versions. No migration
or automatic adoption of arbitrary ordinary tables is offered. Creation first
creates the managed directory, then commits both schemas/scope in one transaction.
An interrupted initial creation may leave detectable incomplete storage: preserve
it for inspection rather than deleting a possibly acknowledged journal.

## Credential lifecycle

Trusted local create_user hashes before staging and acknowledges only after WAL
commit. Credential checking always performs the fixed-policy KDF for a valid
missing or disabled login; missing users cannot authenticate even if a dummy hash
matches. Creation uses a salted hash of fresh random temporary input for that
dummy, wiping the input before filesystem work. This reduces an obvious missing
KDF path but does not establish total-flow timing or HTTP enumeration resistance.

change_password checks the current credential, rejects disabled/missing users,
hashes replacement and increments the epoch in one committed row replacement.
set_disabled is a trusted local administrative operation; changed flags increment
the epoch, while no-ops keep both epoch and history unchanged. Overflow fails
without staging; epochs never wrap. Password input policy still belongs to the
future account service; the low-level 1..1024-byte bound is not a deployment policy.

AccountInfo is current metadata with redacted Debug, not an authorization
capability or session. Pure inspect_account_record checks caller-supplied row data
without hashing, filesystem access or granting permissions. Error formatting
omits identity/input and recursive storage values. Sensitive verifier exports
are confined to explicit private rows/backups; no plaintext password is stored.

## Backups and integration gates

Local backup uses the existing verified EMILYBAK publisher and restores both
schemas, scope, hashes, flags and epochs. Opening restored bytes with a different
expected project is refused. Archives remain plaintext sensitive data requiring
private storage. Compaction retains these invariants for both existing WAL versions.

The current whole-registry archive contains public project data/metadata only.
It does not discover this new separate store. Before network enablement, design
and test complete coordinated account/data capture and restore compatibility;
never silently exclude live account storage from platform backups. Restoring
older epochs must not resurrect future session authority: session incarnation,
revocation and restore semantics need a separate durable design.

Network signup/login, account-service password policy, protected account paths,
blocking-worker/crypto admission, throttling, generic failures, sessions/refresh
rotation, roles and row policies are still pending. No existing API key gains
account administration or changes its data scope. No stage or production gate
is completed by this local foundation.

## Verification

Tests exercise independent scope/epoch models, same-name users in separate stores,
credential changes, disabled/missing checks, no-op/refusal history preservation,
max capacity, epoch exhaustion, corrupt-but-checksummed private records and exact
schema/scope rejection. Both WAL versions preserve old views, reopen and verified
restore; native filesystem tests retain permissions, ownership and no-clobber.
Parser-only fuzzing mutates types/lengths/policy/epoch/login and never runs a KDF.
Results and current verification limits are recorded in testing.md.
