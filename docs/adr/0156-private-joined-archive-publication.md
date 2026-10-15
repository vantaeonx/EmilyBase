# ADR0156: private joined archive publication through the actual selected inode

Status: accepted for experimental trusted native Linux file maintenance only.

## Decision

Add publish_account_file_archive over an immutable successful AccountFileSnapshot.
Select an existing private containing directory through an actual retained handle;
bound the native path/leaf and check containing selection/privacy around source
reads/seeks. Reuse the original storage streamed no-replace publisher with its
bounded write, file fsync, exact readback, atomic no-replace rename and parent fsync.
Keep the actual returned selected file descriptor, never reopen/adopt the filename.

Compare every selected byte against the borrowed canonical encoder with two8192-
byte buffers. Bind original metadata, privacy, single-link identity and visible
name before/after readback; repeat file/parent confirmation fsync and final proofs.
Return only a copied count/identity report after all observations succeed. Wrap
every post-selection failure as ComponentPublicationUnknown. Preserve the selected
or foreign file; forbid overwrite/automatic destructive retry. Existing targets
and pre-selection failures keep the original stage cleanup/no-replace contract.

Add readonly path-bound inspection using the actual input file/parent through
bounded complete-image read, pure nested verification and final unchanged/visible
proofs. Inspection allocates the image and replayed models under existing bounds;
it is not a whole-process memory reservation. Require a regular single-link400/600
file and private containing directory. Debug/count-only CLI expose no data/names.

## Consequences

The native operator must choose a backup directory outside all live inventories;
copied snapshots deliberately retain no source paths/owners for containment checks.
Native paths are filesystem authority, not user/project permission. Observations
are finite under cooperating native ownership, not a namespace lease or sandbox
against the OS owner. Report equality is metadata equality, not backup provenance.
Secret encryption, common root restore, v2 startup/adoption and user file routes
remain separate gates. Existing account/native archive and database/WAL formats
stay unchanged; one internal storage dependency is added without external versions.

Test actual original selected descriptors, exact bytes/privacy and source history,
existing/racing targets, source/parent substitutions, late mutations, injected final
sync failures, readonly aliases/FIFO/oversize/corruption, independent generated
payloads and actual process kills at dedicated write/copy/selection/readback/ACK
boundaries. Test count-only CLI, non-disclosure and failed stdout delivery too.
