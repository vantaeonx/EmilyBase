# Explicit private v6 file-policy catalog

The synchronous AccountStore library now persists complete file policies in the
original private engine. enable_file_policy_catalog explicitly upgrades a fully
valid v5 store to v6: it creates auth_file_policies and changes auth_scope.version
in one WAL commit. Users, password verifiers, session incarnation/clock, row policies
and the current public-admission flag are preserved. The initial empty file catalog
denies file policy access. Open, inspect, sign-in and restart never migrate a store.

V1..v5 remain readable. V1..v4 must explicitly enable their documented intermediate
catalogs before v6. V6 adds one exact table, for nine private tables in total.
Database/WAL/page/EROW and file-policy record versions stay unchanged. Older readers
refuse private v6; preserve a verified pre-migration backup for an explicit offline
rollback to the older snapshot. Removing tables or downgrading the scope manually
is unsupported. This is experimental compatibility, not a stable rolling upgrade.

## Installed data and exact revisions

install_file_policy(context, expected, document) is a trusted native operation.
The caller derives project/native database context from the actual selected file
component. It cannot accept untrusted context assertions as authority. Complete
[bounded records](file-policy-records.md) preserve exact document bytes, including
whitespace, and bind the native identity and actual private commit revision.

Expect0 for absence, otherwise the installed revision. Different content against
a stale revision fails. Identical current bytes with the current revision or its
recorded predecessor return the existing receipt without writing. Unrelated
account/session commits may create revision gaps and cannot turn a stale policy
into a current expectation. No automatic retry follows an ambiguous write result.
Replacement updates the complete single row in one original commit. Runtime
revision exhaustion refuses before writing; full unsigned digits remain intact.

The catalog accepts at most128 distinct native database identities. Replacement
works at capacity. file_policy_receipts validates the complete bounded inventory
and returns canonical identity order, revision/predecessor and digest observations.
Copied receipts have no current authorization or reservation semantics. There is
no deletion/reuse command; install deny rules to disable access. Future retirement
needs a separate tombstone/ABA design before capacity can be reclaimed.

## Current private proof

verify_file_policy_access(token, trusted_now, actual_context) validates installed
records, selects the exact native policy and verifies the current access session.
It returns FilePolicyPrincipal, privately constructed and borrowing the original
AccountStore. Its proof cannot be cloned, serialized or extracted as a model.
The borrow prevents replacing credentials or policy through that store while the
proof is live. Replacement affects the next verification without issuing a token.
An absent catalog/policy or an invalid/revoked session never yields a proof.

check_context admits exact scope before missing-reference or empty-result work.
authorize evaluates the installed [decision model](file-policy-decisions.md).
account_id supplies the genuine owner for a future create adapter. Existing owners
must come from current authoritative native metadata. The trusted adapter must
consume the decision immediately inside the original native operation and retain
its actual private/file owners and source checks. A held borrow does not extend
wall-clock expiry or create a filesystem namespace lease.

This private proof is independent of the separate public-admission flag. A future
user-only adapter must check that flag as well as current credentials/policy and
native ownership. No HTTP/SDK policy administration or native user-file route is
installed by this catalog. Backend file methods still require current service keys.

## Recovery, restore and verification

Complete private open/export/inspection rejects unknown schemas, extra tables,
more than128 file records, malformed keys/metadata/definitions, corruption and a
policy revision beyond the confirmed private LSN. Existing session, row-policy and
admission validation retains exact v6 counts. No corruption is silently repaired.

Verified private/common restoration preserves installed records and their native
binding, revokes old copy sessions and closes copy admission in the original reset
transaction. The source remains usable with its original tokens. The retained
restore test now includes all private versions1..6 under both WALs and checks the
actual original owner lifetime. Native v2 root reopen/capture/independent restore
also exercises a genuine v6 store with the exact native file database identity.

Ten new auth cases include full-u64/capacity/CAS,24 generated command models,
15 semantically damaged engine images and12 actual signal9 kills across both WALs,
migration/install/replacement and staged/caller-received boundaries. The account
worker now asserts signal9 specifically. The private-archive fuzzer synthesizes
v6 records with an independent acceptance oracle, while raw inputs still reach
complete backup validation. No password proof or fabricated principal is fuzzed.

See the source-bound [verification report](measurements/2026-10-20-file-policy-catalog/verification.json).
The subsequent [current-key file-policy gateway](file-policy-write-gateway.md)
connects native operator migration/installation to the actual original common,
private and file owners, with checks at real private commit phases. This supplies
operator coordination; actual native user-file operations, HTTP/SDK files, roles,
signed URLs, global admission, container v2 recovery/upgrade and production acceptance
remain open.
