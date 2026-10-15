# ADR0145: retain the native common file root around observations

Status: accepted for experimental native Linux operator observation/capture.

## Decision

Add an opaque common-root wrapper around the original FileStore, with retained
parent/root/child selection and original readonly WAL/scope files acquired before
component owners. Independently open/lock the original metadata/object children
through retained descriptor paths. Keep their source descriptors alive until the
component owners end; export no writable engine or raw owner handle.

Reuse the common archive restore's child/roster checks rather than duplicate their
rules. File admission keeps original inode, type, private mode, single link, size
and timestamp observations. Original metadata ownership must match the selected
root child/WAL. Compare acknowledged WAL digest, quota, full catalog and complete
physical inventory around each readonly operation. Detected read failure poisons
the wrapper; explicit reopen is required and no source is repaired.

Expose inspection, bounded catalog listing and original coordinated snapshot capture.
The file-root-info CLI emits checked identities/quota/counts only. Preserve native
engine validation's read-only abandoned-tail behavior; capture only acknowledged
history without truncating physical tails. Do not expose common-root write methods
before their actual publication/commit boundaries receive fallible source guards.

## Consequences

Native common-root lifecycle now has an explicit read/capture owner. Existing
standalone FileStore APIs keep their separate original-pair contract. Archive format,
schema, fsync, transaction ACK and external dependency versions are unchanged.

Original capture retains physical object descriptors through its existing copy
checks; the wrapper retains common-root/child/WAL/scope selection around that work.
It does not claim an extended object-descriptor lifetime, namespace lease, whole
memory/FD reservation, HTTP deadline or current user authority. Native ancestors
and Linux descriptor/proc availability remain trusted deployment assumptions.

Admission/read substitutions, exact tail preservation, scope/mode/alias refusal,
owner locks/descriptor return, generated snapshot/charge models, process kills and
actual CLI output define this increment's evidence. Common-root guarded writes,
AccountRoot/current user policy and production acceptance remain open.

The preliminary minimum CLI check exposed cross-test fork/owner-drop interference:
a child can retain an inherited open description briefly before exec. The focused
case passed, while the paired executable reproduced Busy on attempt67. Serializing
fixture creation/drop and peer CLI forks inside this integration executable fixes
test isolation;200 two-thread fresh executables pass. No production lock, WAL
last-clone lifetime, error tolerance or retry behavior is changed.
