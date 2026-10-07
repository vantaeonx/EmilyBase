# ADR0067: bounded synchronous password verifiers

Status: accepted for the password-helper library; account/session integration pending.

## Decision

Keep the database engine original and synchronous. Use the independently
maintained RustCrypto Argon2 library for password cryptography, as ordinary
cryptographic dependencies are permitted. Do not write a password KDF ourselves
or reuse fast API-key SHA-256 hashing for low-entropy passwords.

Use Argon2id version 0x13, 19,456 KiB, two iterations, one lane and a 32-byte
digest. This is the current minimum configuration in the
[OWASP password guidance](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html).
Production cost calibration and independent security review remain required.
Every new verifier receives a fresh 16-byte salt from operating-system entropy.
Compare complete fixed-size digests through subtle's constant-time comparison;
this does not make admission, input rejection or the entire login flow constant-time.

Accept exactly 1..1024 password bytes at this low-level boundary. Borrow those
bytes without copying, normalization or truncation. A future registration service
must define its password policy and own/wipe request buffers separately. This
library does not imply that one-byte passwords are appropriate for accounts.

## Memory and ownership

A cloneable PasswordPool admits one to four concurrent operations. Clones share
an atomic counter; independent pools have independent limits. Reserve before any
block allocation and return Busy immediately when exhausted. No wait queue,
automatic retry, thread creation or async engine is introduced.

Use RustCrypto's explicit
[caller-memory API](https://docs.rs/argon2/0.6.0/argon2/struct.Argon2.html)
with 19,456 1024-byte blocks. Fallible exact vector reservation must produce the
expected capacity; resizing stays inside it. Disable Argon2's alloc/default
features and keep zeroize enabled. This avoids an opaque KDF allocation occurring
before admission. Allocation/randomness/hash failures return typed redacted errors.

The workspace declares its Zeroizing block vector before the admission lease.
[Rust field destruction order](https://doc.rust-lang.org/reference/destructors.html)
therefore wipes/frees that vector before returning the slot. Successful return,
ordinary errors and unwinding use the same ownership path. The intermediate
32-byte digest also has a Zeroizing owner. Caller input and the retained verifier
are outside that ownership; shutdown by abort/kill cannot promise destructor wipes.
No new unsafe code is authored. Dependency internals remain part of supply-chain review.

Reported workspace bytes mean admitted block payload: 19,922,944 per operation,
at most 79,691,776 in one four-slot pool. They exclude allocator rounding,
stacks, caller input, pool metadata and all engine/network memory. Reservation
precedes allocation, so a slot may report admitted bytes before they are live.
This is neither a process-wide RAM quota nor the unfinished engine model budget.

## Record compatibility

EBPWD version 1 is a 72-byte standalone verifier record. It does not change
database page, WAL, backup or cache versions. Fixed little-endian header fields
are magic[8], record-version u16, algorithm u8=2, Argon2-version u8=0x13,
memory-KiB u32=19456, iterations u32=2 and lanes u32=1. Salt[16] and digest[32]
follow at offsets 24 and 40.

Decode checks exact length, magic, version and every policy byte without heap
allocation or hashing. Unknown versions/costs fail closed before costly work.
There is no attacker-controlled PHC parameter parser. Salt/digest are opaque:
changing them may still decode but cannot verify the original password. The
record is not a MAC or checksum; authoritative engine/backup integrity checks
remain separate. Export is explicit through encode; Debug redacts the complete
record and no implicit serialization or Display implementation is provided.

Cost upgrades, stronger-policy verification, rehash and external-format conversion
need an explicit compatible policy/version design. Never silently downgrade or
accept arbitrary stored costs. This foundation is not a stable account format.

## Evidence and remaining gates

Independent raw hashes from
[argon2-cffi 25.1.0](https://argon2-cffi.readthedocs.io/en/stable/api.html)
match synthetic ASCII, Unicode/NUL and 1024-byte inputs at explicit identical
costs. That C implementation is only a test oracle, not a runtime dependency.
Tests cover exact bytes, salt independence, wrong passwords, header mutations,
arbitrary records/costs, shared racing admission, actual held workspaces,
concurrent hashing, wipe observation before deallocation and unwind release.

Synthetic digest bytes survive commit, rollback, reopen and verified restore in
both existing WAL versions. Raw bytes stored in an ordinary table are not an
account registry; malicious cost bytes still fail the auth decoder. Native
allocation observations and source hashes are retained with the
[verification artifact](../measurements/2026-10-07-password-verifiers/verification.json).
The parser-only sanitizer target never hashes fuzz-selected passwords or costs.

Registration/login routes, private per-project accounts, blocking-worker admission,
login throttling, enumeration resistance, sessions/refresh rotation, roles,
recovery/reset flows, transport security and row policies remain pending. Neither
this change nor known-advisory checks complete a security/production milestone.
