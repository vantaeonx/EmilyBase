# ADR0146: guard actual common-root blob/metadata publication boundaries

Status: accepted for experimental native Linux operator publication and CLI.

## Decision

Reuse original FileStore physical publication and metadata transaction behavior,
adding an internal fallible common-root check at BeforeBlob, BeforeCommit,
Committed(original Database) and Verified boundaries. Existing standalone entry
points retain their original native-pair rules without claiming a common namespace.
The public native wrapper exports no mutable engine or raw owner descriptor.

Retain original root/parent/children, WAL/scope and both component owners. Require
unchanged original selection/stamps before publication/commit. After actual metadata
ACK, verify topology and original database/WAL selection, export checked acknowledged
history, then byte-compare the same independently retained original WAL descriptor
with stable private identity/length/timestamps. Advance its stamp only to that exact
committed readback, never to another inode or merely valid alternative. Verify the
original selected-object receipt and final complete root/catalog/quota graph.

Expose native publication and file-root-put. Parse canonical IDs and buffer bounded
redirected input before owner admission. The owner field is trusted native metadata,
not current account authority. Report only IDs/revision/bytes/digest after successful
publication; failed stdout delivery does not undo data or allow replacement.

## Consequences

The original blob fsync/readback/no-replace precedes the original metadata WAL ACK.
Pre-metadata failure can leave a charged invisible orphan; post-ACK state recovers
the committed reference. Source failure poisons the wrapper. Publication uncertainty
and later failure are OutcomeUnknown and preserved, without repair, cleanup or retry.
Invalid input/capacity/duplicate errors preserve usable unchanged state.

No component format/schema/version, fsync/ACK order or external dependency changes.
Native topology/WAL callbacks do not authenticate backup origin, grant user/file
permissions or form a namespace lease. Buffer/replay/descriptor costs remain finite
but are not a server-wide resource reservation or deadline. Linux/proc and trusted
ancestors remain native assumptions.

Common-root guarded rename/removal/quota changes, user download/HTTP/signed URLs,
AccountRoot integration, physical power loss and production acceptance remain open.
Actual source changes, generated state models, killed processes, independent reopen/
restore and CLI receipt delivery define this increment's evidence.
