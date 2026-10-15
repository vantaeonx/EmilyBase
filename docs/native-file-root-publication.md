# Guarded native common-root publication

`NativeFileRoot::publish(file, fresh_object, native_owner, name, payload)` writes
through the original blob-first FileStore transaction while retaining the selected
common root, parent, children, original WAL/scope and both component owners.
The native caller-supplied owner is stored metadata, never an account/session grant.
No current user file HTTP route, signed URL or AccountRoot integration is added.

```sh
emilybase file-root-put root 01010101010101010101010101010101 02020202020202020202020202020202 03030303030303030303030303030303 --owner 04040404040404040404040404040404 --name synthetic < synthetic.bin
```

Canonical typed identities are parsed before stdin/namespace admission. Redirected
binary input is buffered under the existing8MiB object bound before either owner is
acquired. The actual native root then validates persisted quota and complete graph.
Display names remain bounded text and are never filesystem names or HTTP headers.
Existing logical IDs and object names are not overwritten or adopted as new files.

## Actual guarded boundaries

The standalone FileStore keeps its original retained-owner/selected-blob rules and
its separate native-pair contract. An internal fallible check extension adds common
root checks immediately before physical publication, immediately before metadata
commit, after acknowledged metadata commit and after complete receipt verification.
These checks occur at actual original engine/storage boundaries; no alternate WAL
implementation or synthetic commit result is introduced.

Before blob/commit, the original root/parent/child selection, scope file and unchanged
original WAL stamp must still match. Physical publication uses the original bounded
private object publisher, fsync/readback/no-replace rules and retained selected blob.
Only then can the original metadata transaction insert the logical reference and
acknowledge its original WAL commit.

After commit, topology/scope and original metadata/WAL ownership are checked. The
actual acknowledged prefix is exported and verified by the original engine. The
independent original readonly WAL descriptor must still be selected, have exactly
those complete bytes and remain stable around readback using8KiB scratch. Only this
same retained inode's stamp is advanced to the new checked committed observation.
A merely valid replacement, copied inode, foreign tail, public mode or changed
readback cannot become the owner's original WAL. Original receipt checks, final
common selection and complete native graph validation still precede success.

Invalid input/capacity/duplicate refusal leaves WAL/graph unchanged and the wrapper
usable when the source remains unchanged. A detected source failure poisons it.
Source refusal before publication creates no blob. Once blob publication may have
occurred, or after metadata commit, errors are OutcomeUnknown and preserve data.
Original object publication uncertainty is normalized to the same common-root
uncertainty. No automatic repair, overwrite, physical deletion or retry occurs.

## Visibility, quota and delivery

If a durable blob exists but metadata has not committed, it remains an invisible
physical orphan and stays charged. If metadata committed, the acknowledged logical
reference and exact payload recover. Snapshot capture includes charged orphans using
the existing paired archive format, while catalog listing includes committed logical
references only. Successful publication leaves the same root usable for later reads,
captures and independent publications on WAL1 or2.

The CLI reports only project/logical/object IDs, decimal-string revision, byte count
and digest. It omits name, owner and payload. Stdout write/flush is part of process
success. Failed receipt delivery after durable publication preserves the reference;
repeating that ID refuses and never overwrites acknowledged contents. Input buffering
precedes owner acquisition, including oversized-input refusal with an owner already
busy. No output record is emitted for refused input.

## Scope and resource boundaries

Common-root rename/removal/quota updates, streamed user download, HTTP authorization
and coordinated AccountRoot inclusion remain separate work. Original standalone
FileStore mutation/quota methods keep their existing pair contract; this publication
increment does not silently extend common-root guards to them. See
[ADR0146](adr/0146-guarded-common-root-blob-and-metadata-publication.md).

Input buffering, acknowledged WAL export/replay, readback and graph validation remain
bounded allocations with additional retained descriptors; this is not a global heap/FD
reservation, throughput claim or network deadline. Native Linux/proc availability,
trusted ancestors and finite final observations remain. Component formats, private
schema versions, fsync/ACK rules and external dependency versions are unchanged.

## Executed evidence

[Source-bound verification](measurements/2026-10-15-native-file-root-publication/verification.json)
records WAL1/2, empty/8193-byte objects, original history/identity/quota, a charged
orphan, exact payload, source reusability, duplicate refusal and descriptor return.
Invalid names, oversized payload and full physical quota preserve state; separate
reopen and independent paired snapshot restore verify actual acknowledged contents.

Forty-two root/child/WAL/scope/mode/roster changes span blob, metadata-ACK and final
receipt boundaries on both WAL versions. Six further parent replacements cannot
redirect the guarded original commit. Sixteen same-inode WAL rewrite/corruption,
public-mode and foreign-tail cases refuse and preserve changed bytes without repair.
Pre-operation source replacement is direct and creates no blob; later errors are
uncertain and poison. Before metadata commit only the charged orphan appears; after
commit the reference remains acknowledged.

Sixteen actual SIGKILL cases span blob-durable, metadata-acknowledged, standalone
receipt-verified and caller-success, both WAL versions and empty/8193-byte payloads.
Recovered roots show exactly the expected logical visibility/physical charge and
permit the next independent guarded publication. Thirty-two generated publication/
duplicate/quota histories compare independent revision/visibility/charge models
across reopen and paired snapshot restore. Actual CLI processes check relative paths,
binary/empty payloads, wrong/malformed identities, name/scope/quota refusal, bounded
input before owner admission, exact count-only receipts and failed stdout delivery.
Process kills are not physical power-loss evidence or production acceptance.

Final frozen621-source matrix:637 checks per Rust1.99/1.89,312 native files/storage/CLI
and325 database/WAL/transactions/backup. Workspace/fuzz formatting, strict all-target
lint, affected builds and minimum fuzz builds pass. Sixteen new and155 prior native
process kills pass per toolchain. All621 hashes match afterward. Parser/header/encoder/
capture and lockfiles remain unchanged; no new filesystem sanitizer or whole-workspace
acceptance is claimed.
