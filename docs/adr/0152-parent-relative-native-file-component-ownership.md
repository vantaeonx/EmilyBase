# ADR0152: retain original parents for native file components

Status: accepted for experimental native Linux component opening/comparison.
File-capable account runtime adoption remains open.

## Decision

Add bounded private-parent/one-leaf native file opening and finite original-root
comparison. Retain an owned parent duplicate and share the original child/WAL/
scope/store admission with normal path opening. Represent absolute versus
descriptor-relative parent selection explicitly; keep normal absolute behavior.

All existing native topology, blob/metadata commit and payload cursor checks use
the selected original parent. Relative mode validates its private directory rather
than re-adopting an absolute parent path. Component caches/objects remain under
their existing original child descriptors. No owner or mutable component escapes.

Keep supplied comparison refusal inside the checked observation's returned value,
separate from actual original-source failure. Wrong caller metadata must not poison
a healthy store; source failure still does. Acquire no new parent lock, and preserve
the existing open-file-description lifetime of a caller-held lock.

## Consequences

Parent release/moves permit original capture, publication, metadata CAS and
borrowed streaming. Copied old absolute paths remain independent. Selected child/
WAL/scope changes and nonprivate original parent still refuse. Before-commit failure
leaves a charged orphan; after-commit failure preserves the acknowledged reference
under existing uncertain-outcome/poison behavior.

This prepares a component lifetime without creating a partially enabled combined
account/file service. Ordinary startup still refuses file-capable account metadata.
Common ownership, lock/resource admission, actual joined capture/publication/restore
and current file permission remain separate gates. No engine/schema/archive/
credential version, external dependency or durability order changes.

Tests cover original admission boundaries during parent moves, old-path copies,
two-way independent writes after fresh restore, streaming/seek, locks/descriptor
return, bounded invalid input, foreign comparison, namespace substitution, private
parent changes before/after publication, generated revision/name/payload models
and actual killed components. Existing normal-mode/native/core/CLI checks must
remain green on stable/minimum Rust before publication.
