# ADR0069: purpose-bound session token primitives

Status: accepted for cryptographic primitives; durable sessions and HTTP pending.

## Decision

Add synchronous, bounded token issuance, parsing and verifier matching to the
Rust auth library. Keep this independent of AccountStore version1, server routes
and registry backups. It introduces no new runtime dependency or database engine.
Existing project API keys remain a separate credential type and wire format.

Each issuance obtains a fresh 32-byte secret from the operating system. The
caller selects a16-byte family identifier and a scope containing the canonical
32-character lowercase project identifier and a16-byte session incarnation.
These are public routing/context metadata, never user identity or authority.
Incarnation provisioning, persistence and rotation belong to the next durable
protocol. Any16-byte incarnation, including zero, is representable; accepting
metadata is not accepting an authenticated session.

The exact102-byte ASCII text is prefix eba1_ for access or ebr1_ for refresh,
32 lowercase hex family characters, a dot, and64 lowercase hex secret characters.
Parsing checks length first, operates on byte slices and rejects unknown prefixes,
noncanonical hex, Unicode, controls, wrappers and trailing bytes. No input-sized
allocation or normalization occurs. Parsing routing metadata never grants access.
Caller/request buffers still require their own handling and body-size admission.

Store SHA-256 of the fixed106-byte preimage: the NUL-terminated domain
EmilyBaseSessionToken-v1, purpose byte1/2, project16, incarnation16, family16,
secret32. Fixed boundaries avoid concatenation ambiguity and prevent substitution
between purposes or independently selected project/incarnation/family contexts.
This fast verifier is for random256-bit credentials; password hashing retains
its separate salted, bounded Argon2id policy.

Matching receives expected scope selected independently by the trusted caller.
A record cannot select its own authoritative project/incarnation. The full32-byte
secret digest uses subtle timing-safe comparison. Public routing mismatches may
return early; total network-flow timing, enumeration resistance and authorization
are not established by this primitive.

## Versioned private verifier bytes

EBSK version1 is exactly92 bytes, all multibyte numbers little endian:

| Offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | EBSK followed by four zero bytes |
| 8 | 2 | version1 |
| 10 | 1 | purpose1 access /2 refresh |
| 11 | 1 | reserved zero |
| 12 | 16 | project |
| 28 | 16 | incarnation |
| 44 | 16 | family |
| 60 | 32 | SHA-256 verifier |

Decode rejects wrong length/magic/reserved/purpose and unsupported versions
before producing a value. Context/hash payload is opaque, with no independent
checksum/MAC. Authoritative original-engine page/WAL and backup validation must
protect stored records; a valid record header is not proof of authentic content.
No page/WAL/archive format, private account schema or existing credential changes.
Future compatibility changes require a new record version and explicit policy;
unknown versions never fall back to a guessed algorithm.

Issued plaintext has redacted Debug, no Clone/Display/implicit serialization and
one102-byte fallible String allocation. The String owner and parsed/random secret
arrays use Zeroizing. Explicit exposure borrows text; copies made by callers,
request buffers, registers, compiler/cryptographic temporaries, abort or forced
process termination do not acquire a whole-process erasure guarantee. Digest
and scope Debug are also redacted. No raw token enters logs or persistent rows.

## Required next protocol

This is not a session registry. Neither matching nor issuing checks user ID,
credential epoch, disabled state, expiration, revocation or refresh one-time use.
A durable family store must bind these to the private account state, reserve
bounded capacity, atomically replace refresh verifiers and acknowledge only after
the original WAL sync. Concurrent refresh needs one winner; clocks, expiry bounds,
family/generation exhaustion, failure uncertainty and crash recovery need tests.
HTTP workers/rate controls, roles and row policies remain separate pending gates.

Generic backup restore can reproduce an old valid verifier. A future coordinated
account/data restore must durably change the session incarnation before traffic,
so older credentials cannot regain authority. The current generic restore does
not do this: the new regression deliberately demonstrates that gap and verifies
only that an independently selected different incarnation refuses the token.
Do not enable HTTP sessions before migration, coordinated backup/restore and
revocation semantics are implemented and tested. No platform stage is completed.

## Evidence and sources

Independent Python hashlib vectors fix preimage order, purposes and record bytes;
CI reproduces them. Unit/property tests cover canonical grammar, opaque payloads,
all single ASCII-byte substitutions, record header bits, cross-scope matching and
redacted formatting. Both original WAL versions retain only committed verifiers,
preserve old views, reopen and verify backup/restore without storing plaintext.
Pure parser fuzzing runs no RNG/KDF/I/O. Optional native diagnostics measure
matching/decoding and issued-text lifetime, not total heap or latency.
Actual runs and limits are recorded in testing.md and source-bound observations.

The [OWASP session guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
recommends unpredictable credentials, protected server-side state and server-side
expiration/invalidation. It informs the pending protocol; no complete session
security claim follows from token entropy alone. Erasure relies on the pinned
zeroize1.9.1 implementation already inspected for the password helper.
