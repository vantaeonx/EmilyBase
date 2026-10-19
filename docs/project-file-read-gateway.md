# Project-service native file reading

Experimental readonly Rust API over the internally owned components of an explicitly
opened v2 AccountRoot. The caller needs the current project service key. No current
user-file policy, role, signed URL, upload or new HTTP endpoint is introduced.
Service keys belong in trusted backend/operator code.

The API exposes inspect_project_files, list_project_files, project_file_info and
read_project_file. The first three return existing bounded native observations.
The last returns AccountFileRead with copied FileInfo and complete payload bytes.
Its Debug output prints only byte count and revision. It exports no borrowed native
reader, mutable component, descriptor, secret verifier or reusable permission.

## Admission and original selection

On a healthy root, authenticate the current project key before native file lookup,
file graph work or payload allocation. Unknown projects, wrong/master/sibling/old
rotated keys and known projects without a declared file binding deny. V1 roots
cannot become partial file-capable roots through this interface. Select the exact
sorted v2 binding and its already retained NativeFileRoot. Do not discover folders,
close/reopen sources or take owners out of AccountRoot.

Retain original common-root/private/manifest/registry/file-parent checks while the
native model is mutably borrowed. The project-service file operation does not take
an unrelated public database writer. It does retain actual registry/private/native
and common owners already admitted by AccountRoot. Current metadata owner fields
do not authorize a user; closed public-user admission does not grant or revoke this
separate trusted project-service authority.

A failing regression showed that swapping project.json for an equal-byte file
during payload copying was not detected by directory selection alone. The new
per-operation ProjectFileKey guard opens the original project metadata through its
retained project directory using no-follow/nonblocking reads. Before reading it,
require a regular single-link private0400/0600 file within MAX_METADATA_BYTES.
Validate decoded values against the actual live registry model, then retain its
original descriptor, exact initial bytes and identity/size/mode/link/time stamps.
Recheck them around common-root/current-key proofs while the selected payload
reader is alive. All errors use static messages without supplied-value logging.

The guard preserves the existing metadata decoder domain: accepted whitespace and
read-only private files remain valid. This is a per-call metadata selection, not a
claim that older ProjectStore APIs retain a metadata descriptor for their whole
lifetime. A queued read acquires current authority after the root mutex; a prior
key value cannot survive a completed rotation by caching a copied receipt.

## Complete bounded copied payloads

The hard output bound MAX_ACCOUNT_FILE_READ_BYTES is8MiB, matching the native
single-object maximum. The caller supplies a maximum no larger than that. Reject
an oversized actual payload before output allocation; a zero maximum admits only
an empty file. Use fallible reservation and the original NativeFileReader's bounded
8KiB reads, verify exact payload length/EOF and rehash the retained blob before
return. The full graph and common/key checks finish before any copy escapes.

The actual selected payload and project metadata descriptors span the retained,
copied and verified checkpoints. Native finish performs its own final proof;
AccountRoot readiness checks follow after reader unwind. Returned bytes own no
descriptor or future permission. These are finite checks, not a namespace lease.
An error withholds the whole copy. There is no partial streaming response to revoke.

The copied-output maximum is not a total memory, model or physical-I/O reservation.
Native admission may verify the complete selected blob and bounded physical graph
before the caller-size comparison. Existing native inventory/quota limits and
decoded models are additional costs. Metadata-only methods retain their original
native observation boundaries; they do not promise all orphan payload readers for
the whole account operation. Orphans remain physically charged and cannot be read
by guessing their object identity as a logical file ID.

## Executed verification and remaining work

[ADR0167](adr/0167-project-key-native-file-read-gateway.md) records the decision.
[Source-bound verification](measurements/2026-10-19-project-file-read/verification.json)
records commands/hashes, the initial failure and corrected-source results.

Fourteen new server cases cover both WALs, empty/one/four bindings, exact source
bytes and clocks, real owner exclusion, selected reader lifetimes, wrong/old keys,
rotation while a caller waits, cross-project reference isolation, v1/unbound denial,
empty/exact/oversized/max8MiB copies, orphan invisibility, metadata whitespace/modes,
36 late physical changes, parent movement, independent backup/restore,16 generated
models and eight actual signal9 stops, including after API return. Four bounded
worker races replace metadata with a FIFO, symlink, hardlink or broad-mode file
after key admission. A failed fixture initially attempted to rewrite0400 data and
was corrected without weakening the API assertion.

File mutation inside the common root needs checks at actual blob/WAL admission
boundaries and explicit uncertain outcomes. That is a separate increment. Current
user file authorization, streaming, HTTP/SDK file APIs, signed URLs, roles, aggregate
admission, container v2 recovery/upgrade and production acceptance remain open.
