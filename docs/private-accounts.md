# Local private account store

AccountStore is a real Rust library over EmilyBase's original WAL database.
It lives in a separate private local directory and binds its schema to an expected
project ID. Existing public SQL/HTTP project data is not used to store credentials.
The synchronous [retained root service](retained-account-root.md) now holds an
explicit restored private roster. The [embedded HTTP transport](private-http.md)
now provides service-key-gated routes; native binary selection remains pending.

The library provisions users locally, verifies passwords, changes a password after
checking the current one and disables/enables users through a trusted local
administrative operation. Passwords use the [bounded verifier](password-verifiers.md).
Users receive random 16-byte identities and positive credential epochs; changes
advance an epoch, no-ops preserve it and exhaustion refuses a write. Returned
metadata is not a session or permission token.

Each store allows at most 1024 accounts. Login names are strict lowercase ASCII
identifiers up to64 bytes; passwords remain exact bytes with Unicode/NUL support.
Display/email normalization, account-service password policy, self-service signup,
HTTP failure behavior, enumeration resistance and throttling are pending.

Opening verifies the project binding, exact version-selected schema inventory,
one scope row, bounded users, unique identities and every private record. Unknown verifier costs,
nonpositive epochs and invalid private data fail closed. A correctly checksummed
ordinary database is not automatically a valid account store. The module uses
typed engine operations and never builds SQL or filesystem names from a login.

Local backup/restore and explicit WAL compaction use the existing engine protocols.
The current whole-registry backup does not include independently created account
stores. Use the separate [account bundle](account-bundle-format.md) for a common
explicit registry/private capture and [root restore](account-root-restore.md) for
mandatory private reset before selection. Network service attachment remains open. Failed/uncertain initialization remains
inspectable rather than being silently overwritten or discarded.

Do not put this store inside a project's public data directory or expose it through
a generic query route. Future integration must reserve bounded blocking workers,
use one appropriately shared crypto pool, select private paths from authorized
capabilities and coordinate account/data restore with durable session reset. Private
archives still contain sensitive plaintext metadata and salted password verifiers.
[ADR0068](adr/0068-private-project-account-store.md) records the implemented boundary
and outstanding controls. The platform remains experimental.

## Explicit session storage migration

New stores still use private schema1. enable_session_storage explicitly commits
both session schemas, incarnation metadata and private version2 as one original
WAL transaction; repeating it changes no history. Opening either private version
validates complete inventory. Account counts scan auth_users and remain separate
from bounded session-family history. Verified local backup captures four schemas
once migrated. Older readers refuse version2; no implicit downgrade is offered.

SessionRecordInfo/inspect_session_record validate untrusted bounded metadata,
verifier context and clipped time fields without verifying secrets or granting
access. The migration alone grants no access. Local runtime admission was added later
under [ADR0072](adr/0072-durable-local-session-lifecycle.md); coordinated account/data
restore now uses the separate [root protocol](account-root-restore.md); network
integration remains open.


## Explicit clock activation

enable_session_clock(now) explicitly activates private schema3 with a persisted
nonnegative integer-second watermark. Versions1/2 stay readable. Time is supplied
by a trusted local service/operator, never an HTTP client. Equal observations
change no history; forward ones commit and lower ones fail even after reopening.
reset_session_clock(now) changes incarnation/time together so a deliberate time
correction cannot retain the old credential scope. Generic restore does not run
it automatically; coordinated restore must do so before accepting traffic.

The current five-schema inventory is validated, including current-incarnation
family issue-time bounds. The local session lifecycle below now observes this metadata before credential
checks. [ADR0071](adr/0071-durable-session-time-watermark.md) records its separate
compatibility and requested-heap evidence.

## Local durable session lifecycle

After explicit enable_session_clock(now), sign_in checks the real password and
commits a new random family before returning zeroizing access/refresh owners.
verify_access returns a privately constructed borrowed SessionPrincipal after
checking project/incarnation, account identity, current credential epoch,
disabled/revoked state, secret verifier and strict deadlines. The principal borrows
the private owner and cannot be cloned or implicitly serialized. It establishes
an account identity; project capabilities, roles and row policies are separate gates.

Access lasts at most900 seconds, refresh604800 seconds and the family2592000
seconds from creation. Refresh clips both deadlines to the absolute lifetime,
increments the bounded generation and atomically replaces both verifiers. Exactly
one serialized attempt with a refresh secret succeeds; its previous access and
refresh no longer authenticate. Storage uncertainty requires reauthentication,
without automatic retry. logout_session accepts the current refresh credential;
revoke_session_family is explicitly trusted local administration.

Every credential attempt observes trusted service time before checking secrets,
including denied and expired attempts. A later second commits the watermark;
an equal observation is a no-op. A denied attempt can therefore advance the clock
without changing a family. Caller-supplied HTTP timestamps are not supported.
Backward time fails closed, including after restart and restored backups.

All retained families count toward4096 capacity. prune_session_families deletes
at most128 inactive families in one WAL transaction. Invalid bounds fail before
clock observation. Missing/changed/disabled users, obsolete scope, revoked state
or expired refresh permit cleanup; corrupt user data or storage errors propagate
instead of being mistaken for inactivity. A limit is a count bound, not a total
numeric heap or scanning-time quota.

Password changes and disable/enable advance the account epoch, invalidating old
families on every subsequent admission. Explicit scope/time reset invalidates
all older incarnations. Private backup contains these rows; generic restore can
still admit a previously current token under its old incarnation. Run durable
reset before traffic. The existing public registry archive does not capture this
separate store. Separate common-bundle/root capture and restoration are tested;
HTTP routes, bounded server workers,
request throttling, cookie/CORS policy, roles and row policies remain unfinished.

## Private reset before publication

Use restore_private_accounts(archive, target, expected_project, shared_pool, now)
for a separately captured account archive. It validates private schema and project
binding inside the owned staging directory, then resets existing v3 scope/time
or explicitly activates v1/v2. Accounts and epochs remain; old sessions cannot
admit when the final directory appears. No token is reconstructed. Reopen the
published private store normally. The report describes the installed prepared
WAL, including reset/migration commits rather than only the input archive boundary.

Inputs are trusted operator paths/time. Existing destinations are never replaced.
An invalid private archive, callback/commit failure or pre-publication path/sync
failure publishes nothing. A post-rename uncertainty preserves state and requires
inspection before retry. The source archive is immutable. WAL headroom is needed
for reset; there is no automatic compaction or downgrade on failure.

This uses the shared engine restore_prepared protocol under
[ADR0073](adr/0073-private-restore-reset-before-publication.md). Generic engine
restore deliberately has a no-op preparation and retains old metadata. Neither
method makes generic registry archives capture separate accounts. The explicit
[common bundle/root protocol](account-root-restore.md) adds that coordination;
HTTP routes remain unfinished.

## Pure archive inventory and explicit export

inspect_private_account_backup_bytes(bytes, expected_project) checks the complete
private inventory without filesystem access or granting a principal. It returns
private version, account/family counts, optional clock and original-engine report;
all retained family history counts. Unknown/partial schemas or invalid references
fail even when ordinary engine checksums are valid. No reset or implicit migration
occurs. Opening, file backup and explicit backup_image use the same validator.

backup_image returns sensitive committed archive bytes for trusted offline capture.
Never put them in a public SQL/HTTP response. No encryption or total retained-heap
quota is implied. [ADR0074](adr/0074-owned-verified-private-archive-inventory.md)
records owned image lifetime, semantic validation and bounds. An independent
registry backup_image releases temporary data owners before returning; appending
a later private image cannot establish a common boundary. Use
ProjectStore::capture_account_bundle or the explicit root capture, which retain
all owners through private images and final inventory validation. Owned file
publication and coordinated root restoration now execute separately.


The direct restore_private_account_bytes API accepts a bounded sensitive archive
image without creating an intermediate input archive file. It shares the file
wrapper's independent expected project/time validation, complete private schemas,
durable reset/activation and owned no-replace publication. Input bytes and source
credentials stay unchanged; old restored access/refresh tokens are denied before
selection. Generic engine restore_bytes preserves historical private scope and
must not replace the private wrapper. See
[ADR0077](adr/0077-restore-private-byte-images.md). The separate
[root coordinator](account-root-restore.md) consumes this wrapper. HTTP restoration
and whole-process memory admission remain open.

Private byte restoration needs WAL headroom for its mandatory reset/activation commit,
just like private file restore. Capacity refusal leaves the target unpublished; no
automatic compaction is performed.
