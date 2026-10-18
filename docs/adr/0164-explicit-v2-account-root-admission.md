# ADR0164: explicitly admit v2 roots without losing file ownership or bindings

Status: accepted for experimental native library admission and CLI inventory.
Ordinary binary v2 startup and complete v2 rebackup remain separate acceptance.

## Decision

Add explicit AccountRoot::open_v2/open_v2_at and account-file-root-info. Keep default
v1 opening/inspection/capture unchanged. Store a private RootLayout enum containing
the actual complete canonical manifest; never project v2 into a v1 root authority.
Admit private/file counts before opening their models and keep the original manifest
file, exact root/private/file rosters and retained source directory identities.

Open sorted declared NativeFileRoot owners relative to one original files directory,
match persisted database IDs and refuse cross-domain duplicate IDs during checked
registry/private validation. Keep these actual owners in the common root, not copied
receipts or subsequently reopened paths. Component values end before their directory
anchor and common lock. Native readonly selection checking preserves the existing
mutable full-inspection/poison behavior and exports no descriptor or file authority.

Retain normal account/session/SQL operations. Explicitly reject every v1 snapshot/
backup path before staging on v2; a partial account-only result would omit file
bindings. Complete combined capture over internally owned files needs a later
private coordinator with honest split borrows and original-owner lifetime proofs.

## Consequences and acceptance

Normal opening resets no clock/session/admission state. A library caller may
explicitly assemble the existing account router; ordinary server configuration is
still v1. Native metadata bindings are not current user file permission. Public
data writers remain operation-scoped; payload readers are not promised for the
entire active root lifetime. Directory/selection checks are finite, not leases.

Verify actual owners, exact manifest preservation, private/public operations and
session-preserving reopen, native parent moves, legacy and v1-capture refusal,
cross-domain IDs, excessive early admission, changed/malformed sources, generated
models, dedicated actual kills and real binary private inventory. Real TCP tests
exercise explicit library routing and graceful restart; future binary/container
adoption and complete rebackup must be independently accepted. See the
[contract](../explicit-v2-account-root.md).
