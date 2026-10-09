# ADR0116: admitted own-password change under the current private owner

Status: accepted for experimental synthetic-data use.

## Context

Admitted user clients can sign in and use current session-bound rows. Password
changes existed only behind privileged project service credentials. A user needs
to change its own password without selecting a different account or gaining
administrative reset authority. Existing original-WAL epoch replacement already
invalidates all older access/refresh families and should remain the single writer.

## Decision

Add AccountRoot.public_change_password and POST /user/password in private-root
mode. Require one current access Bearer token and current_password plus
replacement_password string fields. No login, user ID, project key, client time or
new session pair is accepted/returned. Native code checks actual root identities
and current opened v5 admission, verifies current access under the retained mutable
private owner, derives that principal's login and invokes the original password
verification/hash/epoch transaction before releasing ownership.

The temporary principal borrow ends before the original mutation, but the same
exclusive private owner spans both. No detached metadata authorizes a later call.
Current access is verified before password work; existing old-password knowledge
is required additionally. Both old and replacement byte limits and original fixed
Argon2id policy apply. Successful replacement increments the original epoch even
if password bytes are unchanged, revoking every earlier family including the
calling one. A fresh sign-in is explicit; the operation never issues a new session.

The HTTP adapter shares original worker/project/socket budgets, body4096/five-second
bounds, exact object-only grammar, wiping secret owners, no-store replies and
fixed-code logging. It checks admission again after waits before trusted server
time. Time can separately commit before a subsequent credential/input refusal.
Only private account state changes; no public table owner or policy lookup is
needed. Closing admission denies change; it does not grant a reset exception.

Extend the actual bounded session grammar/fuzz target with Password. Expose the
same operation through the separate user TypeScript client, requiring explicit
access/current/replacement and retaining conservative unknown remote outcomes.
Lost result must not trigger automatic retry or assume old credentials survived.
This is current-password change, not password recovery or an administrative reset.

## Verification and limitations

Final evidence is appended after all checks. Tests cover both WALs, account/scope
derivation, all-family revocation, independent service rotation, held public data
owners, closed/legacy admission, malformed bodies/limits, delayed current state,
same-token competing changes and change-versus-single-use-refresh races. Generated
raw binary-password histories check the original epoch model. Real TCP received
and observed-unread results are killed/reopened, with a nonempty verified copy.
The expanded real Node client scenarios exercise the actual password route.

The first native preflight failed because its nested fixture parent directory had
not been created. The fixture was corrected after the reproduced filesystem error;
this was not a storage-engine failure or a product corruption finding.

An observed-unread change uses private WAL progress only as a wait cue: that could
be a clock-floor commit. A separate received sign-in with the new password proves
the whole change completed before the kill. This is not an arbitrary pre-ACK or
power-loss proof. Fuzzing checks the actual pure request decoder, not complete
HTTP/header/KDF/session authorization. No signup, roles, reset/recovery channel,
encryption, browser policy, independent audit or production acceptance is added.


Executed:95 Rust checks on each of stable1.99 and1.89.0 (HTTP60, native12,
actual TCP/original HTTP22, documentation1). Ten new regular Rust cases, eight
new generated histories with four password/epoch changes each, and four new
password-result kills per toolchain: two caller-received and two observed-unread.
Node22.22.1 passes34 unit checks and12 actual server checks per toolchain, including
the new SDK method. Workspace/fuzz format and strict lint, both CLI/server builds
and minimum all-fuzz compilation pass on530 frozen source hashes.

ASAN actual bounded session grammar: 10,161,639 inputs in46 seconds,
RSS281MiB under512, max input8192/request4096,1494 initial
seeds including15 new structured cases, no findings. OpenAPI44 operations and489
local references resolve. See
[verification](../measurements/2026-10-09-public-password-change/verification.json).
