# ADR0148: borrow the common root across bounded native payload reads

Status: accepted for experimental native Linux payload cursors and binary CLI.

## Decision

Add NativeFileReader borrowing the original common selection, database, object
owner and shared poison state, with the existing actual ObjectReader. Admit only
committed logical references under the original WAL/full graph. Keep bounded
copied FileInfo and a wrapper cursor, without retaining a payload image. Rust
borrowing prevents root release/metadata mutation while a cursor lives.

Check common selection, original metadata ownership/reference and selected object
before/after bounded8KiB reads and valid seeks. Expose the original ObjectReader's
finite selection/stamp check without full hashing on every chunk. Full hashing and
whole current physical graph validation occur at admission, verify and finish.
Source failures poison both the cursor and root; failed reads clear the attempted
prefix and keep the reported position. Do not reuse inner progress after failure.

Keep invalid seek arithmetic non-poisoning, EOF/empty checks and explicit consumed
final verification. A finish receipt is copied metadata, not proof of EOF/delivery.
Early drop grants no completion claim. Add file-root-cat with bounded raw stdout,
final verification and flush, without a metadata envelope or source mutations.

## Consequences

Actual selected identity/owner lifetime spans native streaming and slow output.
Orphans and removed logical references cannot become downloads. Earlier successful
bytes remain delivered if a later chunk or final check fails. Callers must require
CLI success; no atomic-output, user-permission or network-deadline promise follows.

WAL/graph admission and final graph work still allocate within existing bounds.
Current graph verification is distinct from rehashing all inventory per chunk.
The native trusted-ancestor/Linux/proc/finite-check model remains. Original formats,
schema, durability order and dependency versions do not change.

Executed models, source substitutions, empty/maximum inputs, original descriptors/
locks, compile-fail borrows, killed readers and actual blocked-output processes
verify this boundary. AccountRoot/current user files, policies/signed URLs, global
admission, physical power loss and production acceptance remain separate work.
