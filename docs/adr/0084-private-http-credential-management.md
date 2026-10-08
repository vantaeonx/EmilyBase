# ADR0084: Private HTTP credential management

Status: accepted for experimental service transport; public policy remains open.

Expose password and disabled-state routes over the existing AccountRoot methods.
Keep the current project service key mandatory before body work and recheck it in
the blocking operation. Master/user/cross-project credentials are not substitutes.
This is trusted service authority, not user-role authorization or public reset.

Password change verifies the current password before hashing its replacement.
Both exact UTF-8 inputs use zeroizing owned secret fields, existing bounded KDF
admission and the shared4096-byte/five-second JSON envelope. Invalid/unknown/
duplicate fields, including client time, refuse. No whole-transport erasure is claimed.

Return explicit account metadata only after durable success. A password change or
changed disabled state advances the stored epoch and revokes all older families.
Re-enable never revives old credentials; equal disabled state preserves epoch/WAL.
Retain the existing same-password-change epoch semantics and typed exhaustion.
History remains bounded by the existing family cap; do not silently purge it.

Reuse four-worker admission, private/peer attempt bounds, static errors, no-cache
responses and redacted route logging. Cancellation retains started work as before;
uncertain write outcomes require inspection, never automatic replay.

No engine/token format, public signup/password-reset flow, roles/RLS, family cleanup,
secret encryption, container-root verification or production acceptance changes.
