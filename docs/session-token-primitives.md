# Session token primitives

The Rust auth library can issue and compare context-bound random credentials.
This is a cryptographic foundation. Durable sessions, expiry, refresh rotation,
revocation and HTTP login are not implemented by this module.

```rust
use emilybase_auth::tokens::{TokenKind, TokenScope, issue};

// Synthetic metadata only: real incarnation must be durably provisioned.
let scope = TokenScope::new("11111111111111111111111111111111", [0x22; 16])?;
let (plaintext, verifier) = issue(TokenKind::Access, &scope, [0x33; 16])?;
assert!(verifier.matches(plaintext.expose(), &scope)?);
// Explicit private verifier bytes; never log plaintext or verifier exports.
let bytes = verifier.encode();
# Ok::<(), emilybase_auth::tokens::TokenError>(())
```

Issued text owns one zeroizing buffer with redacted Debug. Explicit exposure
places borrowed text under the caller's control. Avoid copying it into logs;
this does not erase caller copies or provide whole-process memory erasure.
Access and refresh have distinct prefixes and hash purpose tags. Parsing metadata
from either is untrusted routing information, never an account/session principal.
Record decode, metadata parsing and verifier matching use fixed stack payloads.
Malformed, overlong or noncanonical input fails without an input-sized allocation.
No implicit record conversion upgrades unknown versions.

Matching compares the verifier under the caller's independently selected project
and incarnation. It does not validate user state or request permissions. The
complete32-byte hash comparison is timing-safe; total HTTP behavior is unmeasured.
Existing API keys and their current routes retain their separate format and scope.

Only92-byte verifier records enter synthetic original-engine persistence tests.
Both WAL versions exercise rollback, committed replacement, reopen, historical
views and independently verified backup restore. Generic restore retains an old
credential under an old scope; future session-aware restore must rotate and
persist incarnation before accepting traffic. This primitive does not silently
make existing registry archives capture separate account stores.

[ADR0069](adr/0069-purpose-bound-token-primitives.md) defines formats, ownership,
compatibility, pending protocol and verification boundaries. Tests use synthetic
fixtures only. No production or platform acceptance gate closes here.
