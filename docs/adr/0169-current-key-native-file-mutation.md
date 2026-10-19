# ADR0169: current-key file mutations inside the original common root

Status: accepted for experimental native project-service Rust use.

## Decision

Reuse the readonly owned component frame for current key and exact v2 binding
admission, original project metadata selection and final common proof. Invoke its
check from the actual native write guards of ADR0168. Do not implement mutation
as a pre/post-only wrapper, reopen source components or export current authority.

Expose native publish/rename/remove/quota methods. Group borrowed publish input
in ProjectFileUpload; its owner field is metadata, not user identity. Keep native
scope/revision CAS, physical orphan charge and existing no-op behavior. No HTTP,
SDK or user-file permission is introduced by the service key.

Retain the original typed outer guard cause. Once native blob publication or a
commit may have occurred, include failures from the final common proof in a typed
AccountFileWriteUnknown result. Poison the whole live AccountRoot until explicit
close/reopen, even if external metadata is restored. Do not persist that flag,
invent rollback, remove possible data or retry automatically. Early/no-op failures
remain distinct from publication; native staged metadata poisoning remains intact.

## Acceptance and limits

Exercise both WALs, actual phase/owner/reader/key lifetimes, key and scope isolation,
late common/key/native source changes, preserved confirmed/orphan state, whole-root
poisoning after restored metadata, no-ops, invalid/CAS input, generated models,
independent backup/restore and 50 actual signal9 kills including actual API return.
Use the [contract](../project-file-write-gateway.md) and source-bound report.

No persisted format/parser/dependency change, distributed atomicity across unrelated
stores, namespace lease, physical power-loss proof or production acceptance is claimed.
