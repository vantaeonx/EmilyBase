# Private joined archive publication and inspection

`publish_account_file_archive(&snapshot, path)` publishes one successful immutable
AccountFileSnapshot. Use an existing private backup directory outside every live
database/file inventory. The snapshot retains no source path for enforcing that
operator choice. This trusted synchronous native API grants no user permission.

## Publication boundary

Native path/leaf bounds are4096/255 bytes; containing selection/privacy are checked
through an actual retained directory. Source reads/seeks repeat those checks while
the original storage publisher writes bounded chunks, syncs the file, compares full
readback, selects atomically without replacement and syncs the parent. Existing
files, directories and links are never replaced. A failed pre-selection stage is
cleaned only by its original owned identity; foreign entries are preserved.

The wrapper retains that actual selected file through another exact canonical
byte comparison, explicit file/parent confirmation syncs and final metadata/name/
privacy checks. It uses two8192-byte buffers rather than another full archive copy.
Mode is600, single-link; no payload/name/verifier is emitted in the count report.
Copied report getters include manifest/account metadata and bounded totals; they
are not source authority, an open destination or a lease.

Every wrapper failure after selection is ComponentPublicationUnknown. The core's
uncertain selection/durability result maps there too. The target may already exist
and be durable, changed or foreign. Inspect explicitly; never overwrite or blindly
retry at that name. Source account/session/WAL/object state is not changed.

Checks are finite observations, not protection against arbitrary OS-owner changes
after success. Exact bytes are checked against the immutable captured source;
report equality alone does not authenticate origin or prove complete byte identity.

## Readonly inspection and CLI

`inspect_account_file_archive(path)` opens the actual original no-follow/nonblocking
input under a private retained containing directory, requires a regular single-link
600/400 file, bounds the complete image, verifies all nested contents and repeats
actual metadata/name checks. It creates/repairs nothing. Unlike publication's
streamed readback, inspection allocates the bounded complete archive image and
nested models; process memory admission remains open.

```sh
emilybase account-file-verify private-backups/project.archive
```

Success prints nine JSON fields: format, projects, private_projects, file_stores,
references, physical_objects, copied_bytes, archive_bytes and decimal-string reset_at.
Project IDs, display names, payloads and credential records are omitted. Refusal
prints no success receipt. Failed stdout delivery exits nonzero and preserves input.

## Compatibility and evidence

Existing byte formats remain intact. This publishes one backup file; it does not
restore a common root, enable v2 startup or add user upload/download authorization.
Backup encryption, common prepared-owner restore and production gates remain open.

[Source-bound verification](measurements/2026-10-15-component-publication/verification.json)
records exact affected suites and exclusions. Thirteen new regular cases include
ten native cases, three actual CLI cases and16 generated payload models. Twenty-one
actual signal9 kills span prepared/copying/copied/selected/readback/acked/returned
boundaries for empty, one-WAL1 and two-compacted-WAL2 sources. Pre-selection targets
remain absent; later targets contain exact complete bytes. Actual selected output
FDs remain through verification and are absent after report return. Source histories
survive every kill and can produce another fresh backup.

Mutation tests cover three racing targets, six pre-selection parent/privacy changes,
24 late selected-file/parent/mode/content/link/length changes, four injected final
sync failures and12 readonly source changes. They retain original/foreign entries,
withhold success and recover valid copies by independent readonly inspection.
CLI checks real Unicode/relative paths, both native WAL versions, empty/nonempty
copies, count-only non-disclosure, refusal and failed report delivery.

Final affected suites pass800 cases on each Rust1.99/1.89:33 component cases,
12 original file-publication cases,28 registry integration,42 account/new CLI,
325 original engine,315 storage/object/file and45 native CLI.29 ignored entries
are26 parent-invoked fixtures and three separate corpus generators. All648 source
hashes match after final processes close with0; formatting, strict workspace/fuzz
lint, builds and minimum fuzz compilation pass. The byte parser is unchanged;
no new sanitizer/auth/SDK/full-workspace/container claim is made. Earlier complete
hosted0b36669 passes1764/0/43 per toolchain and all five jobs;640 committed source
blobs match. That earlier CI is distinct from this publication increment.
