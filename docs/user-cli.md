# Offline private user administration

The Rust CLI manages users in an existing stopped account root using its current
project service key. Commands acquire the original exclusive owners; a running
server refuses as busy. This is trusted operator provisioning and metadata access,
not public registration, roles or user-token authorization. Use synthetic data.

Privately provision the current service key in a separate file with exact0400/0600
permissions, one hard link and64 lowercase hexadecimal characters plus optional
final LF. Pass only its path as `--key-file`. No key value/password argument or
credential environment fallback is accepted. The shared loader rejects symlinks,
FIFOs, broad permissions, malformed or changing files. Trusted operators control
parent paths; files remain plaintext outside the strict root and its bundles.
See [key-file rules](master-key-files.md) and [offline policies](policy-cli.md).

```sh
emilybase account-user private-root PROJECT_ID --key-file service.key list --limit 100
emilybase account-user private-root PROJECT_ID --key-file service.key \
  create synthetic_user < protected-password-input
emilybase account-user private-root PROJECT_ID --key-file service.key disable synthetic_user
emilybase account-user private-root PROJECT_ID --key-file service.key enable synthetic_user
```

Replace PROJECT_ID with current trusted operator metadata. `protected-password-input`
is a privately supplied input file outside the repository; the CLI does not create
or secure that file. Do not place a literal password in shell commands/history.
Create requires redirected stdin and refuses a terminal before visible echo can
occur. Input is1..1024 exact raw bytes and must end at EOF. A final LF is part of
the password; no whitespace, Unicode, binary byte or newline normalization occurs.
The native engine accepts binary passwords. For users who sign in through JSON
HTTP, provide bytes representable by its existing UTF-8 string password contract.

Password hashing uses the original salted Argon2id implementation and one-operation
pool. The command does not issue access/refresh tokens, change trusted time or reset
existing sessions. A duplicate login refuses; it never replaces the verifier.
Canonical logins are1..64 ASCII bytes: the first is a lowercase letter/digit and
all remaining bytes are lowercase letters, digits, dot, underscore or hyphen.

Create, disable and enable return `{"user":...}`. User metadata contains exactly
`id` (32 lowercase hex digits), `login`, `credential_epoch` (exact decimal-string
u64) and `disabled`. These fields are trusted operator output. Passwords, verifier
records, salts, keys and session credentials do not appear in stdout or errors.
Failures use nonzero exit codes and static typed diagnostics.

List returns `{"users":[...],"next_after":...}` in canonical-login order.
`--limit` is1..128; `--after` is an exclusive canonical-login boundary and need not
identify an existing user. A non-null continuation is the last returned login
when another account exists. Disabled users remain visible metadata. Calls inspect
current state rather than pinning a multi-call snapshot. Neither list nor repeated
unchanged disable/enable writes a private commit.

Disable increments the credential epoch and denies current sign-in/access/refresh.
Enable increments it again and permits a fresh sign-in; old tokens stay revoked.
The original account store enforces overflow and missing-user behavior. No token
or password-reset authority is inferred from a metadata page.

Input validation finishes before root acquisition. While a password pipe waits,
an operator may rotate the project key; the previously loaded key then refuses
when the native root checks current authorization. stdin has no network deadline.
An oversized still-open writer refuses after1025 bytes without waiting for EOF.
Terminating incomplete input leaves both original WALs unchanged. Concurrent
same-login creations have one winner; the loser refuses as busy or existing, and
must not reset the password or retry a changed action automatically.

A lost/failing stdout write may follow a durable account change. Inspect current
metadata and apply only the intended explicit next action; missing output does
not prove rollback. Output is limited to65,536 bytes. Buffers owned by the CLI
zeroize on release; that does not erase source files, pipes, OS caches, terminal
history or external copies.

Verified root backup/restore preserves users, password verifiers, disabled epochs
and v4 policies. It resets session incarnation, so old tokens fail in the copy and
fresh sign-in is required. External key/password input files are not bundled.
See [private roots](account-root-restore.md), [ADR0108](adr/0108-offline-private-user-cli.md)
and [testing](testing.md). Public admission, roles, dashboard, file/realtime services,
Kotlin, load/upgrade/security/resource and production gates remain open.
