# ADR0161: restore the joined archive under one original private v2 root

Status: accepted for experimental synchronous native Linux offline restoration.
v2 live startup, joined CLI and production acceptance remain open.

## Decision

Purely verify the complete existing joined envelope before staging. Prepare a canonical
v2 root.json with the trusted new reset time and the exact registry/private/files
layout. Keep the actual original root locked, manifest and native group descriptors
retained. Compose the existing retained registry, private account and file protocols
through bounded named nesting without closing/reopening prepared owners.

All original component writers span common root selection. Inner writers then finish
their own proofs and release in order; a naive final reopen cannot stand for those
original owners. Capture a bounded independent physical tree proof while all writers
are held:original readonly directory/file descriptors, exact rosters, identity/size/
privacy/link/time stamps and streaming SHA256 hashes. Keep those original readers
and the common lock across unwind and final root/manifest/name/parent-sync proofs.

Use the unchanged storage no-replace publisher. Preserve public data/key metadata,
file catalog/payload/orphan images and exact private records. Each private copy gets
its fresh durable session scope/time and v5 admission closure before common selection.
Return only copied inventory after final proofs. Root publication/final uncertainty
is typed ComponentRestoreUnknown and preserves selected/foreign/nonempty stages.

## Consequences

A complete offline copy becomes available without weakening ordinary v1 startup:
that startup still refuses v2. Native components may be independently opened after
successful return, while explicit future v2 ownership/admission remains necessary.
No current user permission, file authorization, source authentication, encryption,
namespace lease or rollback of acknowledged preparation is inferred.

Separate physical proof caps account for checkpoints/preparation:512 directories,
1024 files,depth6 and384MiB, streamed through8192-byte scratch. Existing source image,
project, file-store and physical object limits remain. These are not global resource
reservations or guaranteed host capacity. Existing formats/dependencies are unchanged.

Verify mixed/empty copies and independent writes, copied token refusal/source token
validity, all writer lifetimes at selection and independent readers during final
unwind, physical changes before/after publication, sync faults, namespace/privacy/
size/depth limits, parent moves, generated rosters and dedicated actual process kills.
See the [contract](../account-file-root-restore.md) and final source-bound evidence.
