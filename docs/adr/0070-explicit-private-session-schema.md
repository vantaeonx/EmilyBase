# ADR0070: explicit private session schema migration

Status: accepted for local migration/validation; session lifecycle service pending.

## Scope and compatibility

AccountStore creation still produces private schema version1 with auth_scope and
auth_users. Opening supports versions1/2 and rejects unknown versions, missing
schemas, changed columns and additional schemas. No automatic migration runs on
open, password checking, account creation or server startup. Version1 stores keep
the same bytes and behavior. Older readers reject version2; downgrade is not an
implicit rewrite. Back up a synthetic local store before explicitly upgrading it.
The database page, WAL and EMILYBAK formats do not change.

The trusted local owner invokes enable_session_storage. It chooses a fresh16-byte
OS-random incarnation before staging, creates both session schemas, inserts one
metadata row and changes auth_scope.version from1 to2 in one original-engine
transaction. Success follows mandatory WAL commit/sync. Cached scope is published
only afterward; a poisoned/uncertain owner cannot use the metadata getter to
report success. Repeating the call preserves the existing incarnation and exact
journal bytes. This method migrates storage; it never signs in a user or grants
session permissions. No server or CLI route is enabled by it.

Version2 requires exactly four live schemas. auth_sessions_meta has non-null
integer primary id, integer version and bytes incarnation. Exactly one row has
id1/version1 and a16-byte incarnation. auth_scope keeps its original schema and
project/dummy verifier, with version2. Account limits remain1024. Count now reads
checked auth_users primary rows rather than subtracting one from the total;
metadata/family history cannot inflate the account count or hide excess users.

## Bounded family record version1

The non-null auth_sessions schema has text primary family (32 canonical lowercase
hex characters), incarnation bytes16, login text under the existing64-byte policy,
user identity bytes16, positive i64 credential epoch/generation, nonnegative
created/issued times, access_until/refresh_until/absolute_until times, EBSK access
and refresh verifier bytes92 each, and revoked Boolean.

Times use integer seconds, a fixed900-second access window,604800-second refresh
window and2592000-second absolute family lifetime. absolute_until equals
created plus absolute lifetime; issued lies from created inclusive to absolute
exclusive. Access/refresh ends equal issued plus the smaller of their window and
remaining absolute lifetime. The decoder bounds signed arithmetic and clips
before addition. A reproduced near-i64::MAX regression rejects an initially
valid clipped interval; the corrected calculation admits it without overflow.
Clock selection, backward-clock handling and runtime expiration enforcement are
pending service requirements, not properties of record inspection.

Cap each store at4096 family records, including revoked/expired/old-incarnation
history. With1024 accounts and two metadata rows the maximum5122 rows remains
under the independent engine10000-row bound. No whole-memory reservation is
claimed. Pure inspection checks exact types/lengths, canonical login/family,
positive counters, deadlines and both verifier purposes/project/incarnation/
family contexts before returning redacted metadata. Hash payload remains opaque:
inspection never verifies a secret and is not an authorization capability.

Opening validates every checked family row and resolves its login against the
current private user table. Identity must match; stored epoch may equal or precede
current epoch, never exceed it. Older epochs/incarnations may remain invalidated
history without making the database corrupt. The future admission service must
check current epoch, disabled/revoked state, incarnation and expiration on every
credential attempt; accepting a historically well-formed record does not revive it.

## Pending lifecycle and restore gates

No public family insertion, sign-in, refresh, logout, pruning or principal API is
implemented in this increment. These require bounded issuance and collision
attempts, one atomic refresh winner, generation/epoch exhaustion, current-account
checks, server-selected clocks, failure uncertainty and process-kill tests.
The private store's ordinary backup now captures all four schemas consistently
in the same mandatory WAL; existing whole-registry archives still do not discover
this separate store. Restored incarnation is preserved as data, not automatically
rotated. Coordinated account/data restore must change it durably before traffic,
so the already demonstrated old-token resurrection gap remains explicitly open.
Numeric model/cache/staging/worker admission and production gates remain pending.

## Evidence

Both WAL versions preserve users/passwords, old v1 snapshots, one-commit upgrade,
no-op journal equality, reopen, compaction and independently verified four-schema
backup/restore. Forced process kills at staged upgrade and flushed post-commit
acknowledgment recover exactly version1 or complete version2, with unchanged
user identity/epoch and independently restored recovered backup. The subprocess
harness retains its shared filesystem/launch mutex and bounded acknowledgment.
This is not physical power-loss or a new mid-fsync fault campaign.

Actual4096/4097-row histories verify the cap; capacity fixtures obey the engine's
separate256-event transaction limit. Missing metadata, versions, lengths, schemas,
foreign identities/epochs, wrong verifier contexts and complete row shapes fail
with valid engine checksums. A512-case independent clipped-deadline model spans
the signed time range. Parser-only fuzzing and final checks are recorded in
[testing.md](../testing.md); no session/HTTP milestone is closed.
