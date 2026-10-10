# Native paired file archive publication

`publish_file_archive(&FileSnapshot, operator_path)` writes one complete checked
[paired archive](native-file-archive-format.md) through the original storage reader
publisher. It retains the destination parent before copying, creates private stage
bytes, enforces exact source length/EOF, fsyncs the file, performs full byte readback,
selects without replacement and fsyncs the parent. Existing destinations refuse.
The actual selected descriptor and parent stay retained through final checks.

Final verification checks regular-file type, private0400/0600 mode, single link,
bounded length and stable inode/size/timestamps around readback and decoding. The
selected visible entry and retained parent must still match. Complete paired
schema/scope/quota/reference/hash checks run against readback bytes, followed by
exact byte comparison against the original snapshot's canonical borrowed encoder.
Another independently valid archive cannot substitute for the expected snapshot.

The original publisher's uncertain result, or any error after selection, returns
OutcomeUnknown. The caller must inspect the target before choosing a next action;
there is no overwrite, automatic retry, source poison or selected-file cleanup.
Earlier source capture and publication are separate native operations on immutable
bytes. The copied FileArchiveReport gives project, metadata report, quota, reference
count and physical archive report, never a retained capability or namespace lease.

`inspect_file_archive(operator_path, expected_project)` is readonly. It opens the
leaf without following symlinks, checks private regular-file/single-link/size rules
before allocation, holds the actual descriptor/parent, and checks stability/visible
selection around complete paired verification. Debug reports omit names, content
and identifiers. Checksums do not authenticate backup origin or current authority.

The trusted native operator must choose output outside live object inventories.
FileSnapshot owns immutable bytes without source-directory descriptors and cannot
classify arbitrary destinations against live/moved sources. No path from an
untrusted request is accepted by a server route here. Native ancestor trust remains.
The encoder streams with8 KiB scratch, but final semantic readback materializes one
bounded archive and transient original metadata replay. Maximum encoded-size bounds
are not a global memory reservation; no maximum-size RSS improvement is claimed.

See [ADR0140](adr/0140-owned-paired-file-archive-publication.md). This is standalone
native common backup publication, with no common restore, AccountRoot, user policy,
signed URL or production readiness claim. Component byte formats/schema/fsync/ACK
are unchanged.

## Executed checks

[Source-bound verification](measurements/2026-10-10-file-archive-publication/verification.json)
covers native WAL1/2 and empty/8193-byte objects with an additional charged orphan.
Independent byte/path inspections match the source metadata, exact graph and physical
charge. Existing target bytes remain unchanged on no-replace refusal;0400 readonly
inspection works. Symlinks, hardlinks, public mode, directories, wrong project and
oversized private sparse files refuse. The size rejection leaves sparse length intact.

Five selected-source substitutions cover identical-byte new inode, public mode,
same-inode corruption, moved/replaced parent and another valid same-project archive.
All are uncertain and preserved. Rewrites after reading and after semantic decoding
are detected without repair. Twelve actual SIGKILL cases span prepared-before-write,
durably-selected-before-final-result and caller-success, WAL1/2 and empty/8193-byte
objects. Prepared targets remain absent; selected/acknowledged targets independently
verify, and the recovered source still permits an independent next metadata write.
The preceding83 native kill cases rerun; no common restore kill matrix is implied.

Final affected matrix:589 checks per Rust1.99/1.89,268 native files/storage/CLI
and321 database/WAL/transactions/backup. All605 source hashes matched. Formatting,
strict workspace/fuzz lint, explicit builds and minimum fuzz builds passed. The12
new and83 preceding native kills passed per toolchain. External package versions
are unchanged. Header/encoder/fuzz target/capture and decoder function body remain
unchanged from the preceding finite sanitizer campaign; no new campaign is claimed.
