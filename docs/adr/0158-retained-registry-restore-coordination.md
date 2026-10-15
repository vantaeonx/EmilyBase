# ADR0158: retain every prepared public writer through enclosing restore work

Status: accepted for experimental trusted synchronous native Linux coordination.
Complete account/file restore and runtime adoption remain open.

## Decision

Add with_restored_registry_backup_bytes_at over a complete original registry image,
explicit private parent and bounded fresh leaf. Reuse existing component preparation
without altering ordinary path-based restore. Retain the actual selected stage and
original registry/project/data owners; acquire all public writers before the first
prefix. Use checked full canonical-image capture and exact final history hashes.

Retain independent actual data-directory/WAL readers, require original physical
length/metadata/single-link/privacy/name and compare complete bytes in bounded
chunks. These physical proofs are necessary because acknowledged export legitimately
omits an unacknowledged tail. An isolated new-protocol regression demonstrates that
history-only comparison accepts an appended tail during work. Ordinary recovery's
acknowledgment semantics remain unchanged.

Select the complete child atomically without replacement and sync the retained parent
before work. Keep every writer and original WAL through nested work, final source
hashes and a last original-selection observation. Add an internal final-check hook
to the shared checked capture; existing callers use a no-op hook and preserve their
contracts. Work gets copied public report metadata only. Withhold results if any
final proof fails; propagate callback error values through both nested result layers.

Every postselection error is RegistryRestoreUnknown. Never sweep failed private
stages, overwrite selected targets, adopt foreign names or silently retry. Existing
targets refuse without invoking work. Permit independent same-scope registry copies;
the future common coordinator owns aggregate admission, account reset and its v2
root manifest. No runtime user/file grant or production stage is implied.

## Consequences

The original public writer lifetime can span private account/file preparations and
enclosing root selection without a close/reopen interval. Repeated hashes/readback,
source/copy buffers, replayed models and additional per-project descriptors cost
resources;128-project/image caps are not whole-process memory/FD reservations.
Native OS-owner trust and finite-observation limits remain. Formats/parsers/external
dependencies do not change. No new sanitizer or physical power-loss claim follows.

Verify actual nested/maximum owner lifetimes, metadata/identity substitutions,
physical tails, callback result withholding, final same-byte root replacement,
pre/post-selection sync faults, generated independent models and actual process kills.
See the [contract](../native-registry-restore-coordination.md) and source-bound matrix.
