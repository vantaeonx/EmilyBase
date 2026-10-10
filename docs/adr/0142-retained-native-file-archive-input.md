# ADR0142: retain the original native archive input through common restore

Status: accepted for standalone native Linux operator APIs and CLI.

## Decision

Keep the actual readonly input file and parent descriptors after bounded private
regular-file admission and immutable image reading. Reuse the existing archive
path/metadata checks and bounded image reader. Extend common restoration with a
fallible input-check callback before staging, immediately before root selection
and during final postselection verification. Byte-slice restoration keeps its
existing behavior with an infallible callback.

Recheck exact source bytes using bounded scratch plus original identity, mode,
single-link, size, timestamp and visible file/parent checks. Do not reopen an input
path or accept an independently valid replacement as the original. Keep the one
immutable archive image and original restored owners through common publication.

Expose native file-archive-verify and file-archive-restore CLI commands, each with
an explicit canonical project. Print counts/identities only, with acknowledged
transaction numbers as strings. CLI success includes stdout flush. A failed report
after restoration does not undo a selected root and does not allow replacement.

## Consequences

No archive/component/private-schema version, fsync or transaction ACK rule changes.
The CLI adds one existing local crate dependency, with no new external package
version. Native direct paths are not server user input or an authority grant.

Preselection input failure leaves the common name absent and preserves nonempty
stages; postselection failure is uncertain and preserves the selected root. The
original source is readonly throughout. Existing destinations are never overwritten.
Original owners and input descriptors close on every return; process kills release
their locks without recursive cleanup. Final checks remain finite observations,
with trusted native ancestors and no namespace lease or global memory reservation.

AccountRoot integration, current file policies, signed URLs, authenticated backup
origin and production acceptance are independent gates. Parser sanitizer evidence
does not certify filesystem restoration or physical power-loss behavior.
