# Admitted own-password change

In explicit account-root mode, POST /v1/projects/PROJECT/user/password requires
one Authorization: Bearer current access token and application/json:

```json
{"current_password":"caller supplied current password","replacement_password":"caller supplied replacement"}
```

These are field-shape examples, not default credentials. The operator must have
provisioned the user and explicitly opened private v5 admission. The legacy router
has no private account store. Service/master/refresh credentials do not replace an
access token, and there is no login/user-ID field. The Rust private owner derives
the exact current session account before verifying its old password.

Passwords preserve exact valid UTF-8 bytes without trimming or normalization.
Each is1..1024 bytes; the entire escaped object must fit4096 bytes. Unknown/duplicate
fields, positional arrays, client time and identity overrides refuse. The original
five-second body deadline, shared four workers and private project/real socket-peer
limits apply. Native Rust callers can use original bounded byte passwords; JSON
and the TypeScript client cannot represent invalid UTF-8 binary passwords.

Selected filesystem identities, current admission and current access purpose,
project/incarnation, family generation, account epoch, disabled state and expiry
are checked under the actual retained owner. A change during body/root waits is
observed before mutation. Closed admission refuses before consulting trusted time.
There is no public table/policy lookup or unfiltered data authority in this action.

The original Argon2id helper verifies current bytes, hashes replacement with a new
salt and commits the password record plus incremented credential epoch in one
original WAL transaction. Epoch exhaustion refuses. Replacing with identical bytes
still increments the epoch. All older access/refresh families become invalid,
including the calling access token and sessions on other devices. They need not be
physically deleted for revocation to apply. A concurrent refresh or same-token
password change has one current transition winner.

Success200 returns only id, login, credential_epoch as an exact decimal string,
and disabled. No token pair is returned; log in explicitly using the replacement.
All replies are no-store. Errors/logs contain fixed codes/shapes without passwords,
tokens, bodies, IDs or raw filesystem paths. Invalid current credentials refuse401;
invalid byte/input shape400; uncertain storage/time/worker results use the existing
conservative session failures. Private trusted time can commit separately even if
the password operation later refuses, without changing the public WAL.

A lost result may follow a completed password/epoch commit. Do not automatically
retry with the old access or infer rollback. Try an explicit fresh sign-in through
the original bounded flow using the intended replacement; if credentials are
uncertain, use trusted operator inspection and the existing recovery workflow.
The SDK method changePassword(access, current, replacement, options) preserves this
explicit behavior and returns UserInfo. It stores no credentials and reports remote
errors conservatively as unknown. This is not an email recovery/reset service.

Both native/HTTP and real TCP tests cover normal/refusal/concurrent/restart/copy
boundaries. Received results and separately observed complete unread results are
distinct test outcomes; neither proves hardware power-loss durability. Signup,
roles, password recovery, browser/device policy, independent security audit and
production acceptance remain open. See
[ADR0116](adr/0116-admitted-own-password-change.md) and [OpenAPI](openapi.json).
