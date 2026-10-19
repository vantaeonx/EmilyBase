# ADR0167: retain current project metadata and original payload for native file reads

Status: accepted for experimental readonly project-service Rust access.
No user-file policy, upload, new HTTP endpoint or file mutation is accepted here.

## Decision

Add native AccountRoot methods for scoped inspection/list/reference lookup and
complete bounded copied payloads. Require the current service key, exact original
v2 file binding and internally retained native component. Deny unbound/v1/foreign
requests without native payload work. A returned AccountFileRead is sensitive
copied data, not a capability, live reader, descriptor or current user permission.

Keep the actual selected NativeFileReader through common-root and current-key
checks. Read at most8MiB through its8KiB bounded cursor and verify complete length,
EOF, retained blob and graph before returning bytes. Check full AccountRoot readiness
after nested borrows end. Retain source models and owners without reopening them.

First reproduce the equal-byte project.json replacement failure in a regression.
Then add a private borrowed ProjectFileKey guard selecting original metadata through
the actual retained project directory. Require bounded regular/private/single-link
input before reading; compare decoded canonical values with the live registry model,
then retain exact initial bytes, descriptor and stamps through the file operation.
Preserve the accepted metadata encoding domain and0400/0600 modes. Do not promote
this guard into a detached user authorization receipt.

## Consequences and acceptance

No source write, clock advance, session reset or format/parser/dependency change.
Output bounds are separate from native full-graph audit/model costs. Metadata-only
observations and borrowed payload lifetimes remain distinct. Logical orphan
invisibility and current key rotation apply before returning any copied data.

Verify actual original owners/descriptors, complete output withholding, both WALs,
current/foreign/queued-old keys, scope isolation, output bounds including max8MiB,
late physical changes, metadata special-file races under a process deadline,
same-byte replacement, parent moves, backup/restore, generated models and actual
signal9. See the [contract](../project-file-read-gateway.md).

Mutation needs a later common-root/current-authority check at the actual blob and
private-WAL admission boundaries, plus uncertainty/poison handling after publication.
A before/after wrapper alone must not be presented as that implementation. Existing
native filesystem-owner write APIs retain their prior contract.
