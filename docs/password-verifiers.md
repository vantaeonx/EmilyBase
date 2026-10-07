# Password verifier foundation

The Rust auth library now provides a real synchronous Argon2id helper. It does
not expose registration/login HTTP routes or store accounts/sessions. Existing
high-entropy API keys retain their separate SHA-256 digest design.

```rust
use emilybase_auth::password::{PasswordDigest, PasswordPool};

let pool = PasswordPool::new(2)?; // Retain and share clones across the intended scope.
let digest = pool.hash(b"synthetic-example-password")?;
let bytes = digest.encode(); // Explicit private storage export; never log this.
let restored = PasswordDigest::decode(&bytes)?;
assert!(pool.verify(b"synthetic-example-password", &restored)?);
# Ok::<(), emilybase_auth::password::PasswordError>(())
```

This is a library example, not an account provisioning command. Use synthetic
data while the wider recovery/security gates remain open. A stored salted
verifier is sensitive because it enables offline password guessing if disclosed.

Inputs contain 1..1024 exact bytes. Unicode and NUL are supported without
normalization; overlong or empty input is refused before admission. Password
policy and request-buffer erasure belong to the eventual account service.
Malformed/unknown 72-byte records fail before a KDF operation. Wrong passwords
return false; exhausted shared capacity returns Busy, not false. Callers must
distinguish overload from a failed credential and avoid unbounded retries.

Each admitted operation owns 19 MiB of zeroizing blocks, with one to four slots
per pool. Clones share the limit. Independent pools do not share it: create one
appropriately scoped service pool instead of constructing one per request.
The module owns no background threads. Future Axum integration must also reserve
bounded blocking workers and release workspaces correctly when requests cancel.

The 72-byte record has a fixed version and fixed Argon2id costs, a random 16-byte
salt and a 32-byte digest. No checksum/MAC is claimed for the standalone record.
Authoritative pages/WAL and backup checks still supply their existing integrity
boundaries. Decoder refusal is not a substitute for private account storage.

[ADR0067](adr/0067-bounded-password-verifiers.md) records cryptographic sources,
memory ownership, compatibility, evidence and pending controls. Native diagnostic
observations measure requested block payload, not whole-process memory or login
latency. The project remains experimental and not production-ready.
