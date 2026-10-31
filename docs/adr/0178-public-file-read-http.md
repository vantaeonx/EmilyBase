# ADR0178: access-only bounded file read transport

Status: accepted, 2026-10-31. Scope: account-root HTTP mode.

## Decision

Expose POST user/files/list, user/files/info and user/files/read beneath the
existing /v1/projects/{id} prefix. Reuse the admitted current-user middleware,
its original peer/project rate budgets and four shared worker permits. Require
exactly one Bearer access credential; do not accept service/master keys, refresh
credentials, X-EmilyBase-Access or query credentials as user authority.

Require strict object-only application/json, a4096-byte actual body bound and
five-second deadline. Decode only after body/root waits and current admission
recheck; observe trusted service time afterwards. Invoke the existing synchronous
public_list_files/public_file_info/public_read_file in spawn_blocking under the
original Root. Those methods reauthenticate and consume actual current installed
policy under the original private/native/common owners. No copied middleware or
metadata observation authorizes file access. Syntax validation itself is pure.

Use a small metadata DTO: logical ID, display name, payload_bytes and decimal
string revision. Do not return physical object IDs, owners or digests. Hidden
and missing info both return null. Hidden and missing payload both return the
same403/code. Policy-filtered lists may be empty. Full binary responses use
application/octet-stream and nosniff, with no display-name header or disposition.
No partial streaming response escapes before final native checks complete.

## Bounds and failure

The caller's maximum_bytes is an integer in0..=8MiB; insufficient permitted read
capacity returns413 JSON with no payload prefix. It cannot bypass native policy.
Current session refusal is401, native read denial403, invalid request400 and
source/catalog/storage refusal503, using static codes rather than Rust error
contents. Every account-router result remains no-store/no-cache. Logs retain
only existing static route/method/status/timing labels.

An in-flight body or root wait cannot retain earlier admission/session/policy.
Body cancellation releases its permit. Native blocking work retains its permit
and owners through completion even when the requesting async task is cancelled,
under the pre-existing shared scope contract. File methods themselves can advance
the separate private authentication clock; these routes make no native file write.

## Boundaries

This adds only current-user list/info/download. Upload/rename/remove HTTP,
SDK file methods, operator HTTP policy administration, signed URLs, Range,
resumable/streamed uploads and reclamation remain separate work. Existing trusted
native operators can configure policy/quota; there is no permissive HTTP default.
Legacy v1 roots do not implicitly gain native file components or migrate catalogs.
No stored format/schema/dependency changes. The new pure grammar gets unit and
sanitizer fuzz checks; genuine v2 roots exercise delayed credentials, bounded
HTTP, raw TCP binaries, independent restore and graceful restart on both WALs.
No power-cut, production readiness or whole-process response-memory reservation
is claimed; already delivered bytes cannot be revoked by a later policy change.
