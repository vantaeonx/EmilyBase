# ADR0087: Bounded private user metadata pages

Status: accepted for experimental trusted service transport.

Add synchronous AccountStore::list_users and retain the existing project gate
through AccountRoot. POST /v1/projects/{id}/auth/users/list accepts strict JSON
with a required integer limit1..128 and an optional exclusive after login/null.
The cursor is a canonical bounded ASCII login, not an encoded token or path;
it may name a gap. Unknown/duplicate fields and client time refuse. Current
project service authority is required before body work and again at execution.
The common body/worker/rate/no-cache/static-error rules remain unchanged.

Use the original checked primary-row cursor. Retain at most128 AccountInfo values
and validate consumed complete private records, including one lookahead and any
inclusive boundary record, before returning a page. A corrupt consumed record
returns an error, never partial metadata. Bound input before allocating the page.
MAX_ACCOUNT_PAGE is independent of the session-cleanup bound. Full decoded-model,
derived-index and whole-process resource admission remain separate work.

Return explicit users metadata only: ID, login, credential epoch and disabled.
Return next_after as the last emitted login only if the current read sees another
record, otherwise null. Do not serialize private verifiers, sessions, families,
incarnation or whole storage objects. AccountPage Debug is redacted; metadata is
not a detached authorization proof. No total/remaining count is promised.

Each read observes current state and retains no snapshot across requests. Inserts
behind a previous cursor may be absent from that continuation; newer entries ahead
can appear. The API neither hashes a password nor observes session time and does
not commit WAL. This is a trusted operator/service view, not public directory
search, self-service signup, user SQL authority or row-policy enforcement.

Tests cover private versions1/2/3 with WAL1/2, actual128-row boundaries over130
users, maximum-length names, exact metadata, gap/end/invalid cursors, current
mutations between pages, corrupt lookahead and eight independent generated
inventory models. HTTP cases cover strict inputs, sibling isolation, no clock/WAL
changes and key rotation while an admitted body waits. Real TCP restart cases
preserve metadata and rotated service authority without private/public WAL writes.
The common native/container lifecycle checks the new endpoint as well.

Early test drafts needed mutable WAL-export fixtures, distinct synthetic IDs after
enlarging the inventory, and a correctly bounded reopen expectation. Those fixture
issues were corrected before final checks; no engine behavior was weakened.
No stored format, durable acknowledgement, public policy or production gate changes.
