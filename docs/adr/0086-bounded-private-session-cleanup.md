# ADR0086: Explicit bounded private session cleanup

Status: accepted for experimental trusted service transport.

Expose the existing original-WAL inactive-family cleanup through the retained
AccountRoot owner and POST /v1/projects/{id}/auth/sessions/prune. Require the
current project service key before body work and again inside blocking execution.
Master/user/other-project credentials grant no cleanup authority. An undeclared
private store refuses without provisioning directories.

Strict JSON contains only a required integer limit from1 through128, sharing the
exported MAX_SESSION_PRUNE bound with the synchronous account implementation.
Reject malformed/unknown/duplicate/oversized input and invalid limits before
private clock advancement. Read trusted time from the server clock only. Reuse
the four-worker, private/peer rate, body timeout, no-cache and static error rules.

Remove at most the admitted count in one deletion transaction. Eligible families
are revoked, refresh-expired, or fail current account/incarnation authority.
Access expiry alone does not expire refresh authority. Corrupt state remains an
error rather than being reclassified as inactive. Return only {removed: count}
after durable success, without user/family identifiers, digests or tokens.

Clock observation is a separate durable operation. Zero removals can still persist
a forward watermark; repeating an empty cleanup at the same time changes no WAL.
Deleting rows releases family capacity but appends history: this is not byte-space
reclamation or an automatic compaction policy. Ambiguous writes require inspection,
not automatic retries, background deletion or silent reset.

Two in-process cases cover bounded deletion, revoked/stale-epoch/expired refresh,
access-expired but refreshable retention, exact sibling/public WAL preservation,
invalid input before time work and zero-removal clock semantics. An eight-case
independent count model verifies batch limits, active authority, WAL1/2 and exact
inventory across reopening. A real TCP two-cleaner race removes the inactive
families once; received-ACK SIGKILL and reopen preserve removal and the active
family on each WAL version. The common native/container lifecycle now kills once
after cleanup acknowledgement as well, increasing that scenario to six kills.

No storage, WAL, token, account, bundle or root format changes. No public signup,
user SQL roles/RLS, automatic scheduler, encrypted secrets, whole-process resource
admission or production acceptance is introduced.
