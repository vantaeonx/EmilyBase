# Local private account store

AccountStore is a real Rust library over EmilyBase's original WAL database.
It lives in a separate private local directory and binds its schema to an expected
project ID. Existing public SQL/HTTP project data is not used to store credentials.
The server does not yet attach this library or provide account routes.

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
stores; combined platform capture/restore must be designed and tested before
connecting them to live server projects. Failed/uncertain initialization remains
inspectable rather than being silently overwritten or discarded.

Do not put this store inside a project's public data directory or expose it through
a generic query route. Future integration must reserve bounded blocking workers,
use one appropriately shared crypto pool, select private paths from authorized
capabilities and define durable session/revocation/restore behavior. Private
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
access. No runtime family issuance, refresh, revoke or principal API is enabled.
Current-state/clock/expiry enforcement and coordinated restore rotation remain
open under [ADR0070](adr/0070-explicit-private-session-schema.md).


## Explicit clock activation

enable_session_clock(now) explicitly activates private schema3 with a persisted
nonnegative integer-second watermark. Versions1/2 stay readable. Time is supplied
by a trusted local service/operator, never an HTTP client. Equal observations
change no history; forward ones commit and lower ones fail even after reopening.
reset_session_clock(now) changes incarnation/time together so a deliberate time
correction cannot retain the old credential scope. Generic restore does not run
it automatically; coordinated restore must do so before accepting traffic.

The current five-schema inventory is validated, including current-incarnation
family issue-time bounds. These are local metadata APIs, not implemented session
admission or permissions. [ADR0071](adr/0071-durable-session-time-watermark.md)
records compatibility, requested-heap evidence and required lifecycle integration.
