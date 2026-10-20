# Current-key file-policy writes under original owners

The experimental synchronous AccountRoot library can explicitly enable the private
v6 catalog, observe the selected native store's installed policy and install or
replace it. enable_file_policy_catalog, project_file_policy and
install_project_file_policy require the current project service key, a declared v2
file component and that project's real private AccountStore. The adapter derives
the native database ID from the internally owned component; the caller cannot
supply a database identity. Existing v2 manifests require private membership for
every native file binding. Missing bindings/owners refuse without provisioning or
changing the existing manifest format.

Opening a root, inspecting files, reading policy metadata and restarting do not
upgrade private schemas. The [catalog migration](file-policy-catalog.md) still
requires v5, is atomic, preserves admission/sessions and does not grant user access.
Exact document bytes, revisions, CAS/predecessor retries and capacity retain that
catalog's contract. Observation returns only the selected actual database's copied
receipt. A receipt is metadata, never an authorization, namespace lease or future
reservation. Catalog/format versions and dependencies do not change in this step.

## Actual private commit coordination

The original file-component frame now lends its optional original private owner
alongside the actual native root and its common/current-key check. It never opens
a replacement source, acquires a second directory lock or invents a public-data
writer reservation. The actual common, registry, private, native and retained key
metadata owners span the operation. Policy administration also retains a readonly
original private WAL descriptor. It first proves its actual AccountStore directory,
pins the selected WAL inode without a second writer lock, and repeats that selection
check through phases and final enclosing checks. Legitimate append may change WAL
length/timestamps; the guard does not adopt another inode. This follows a reproduced
private-WAL replacement that otherwise allowed a write to the unselected old journal.
Readonly/backend mutation users still enter
the same frame with unchanged admission and no implicit provisioning/migration.

AccountStore exposes guarded migration/install variants. A trusted callback runs
at BeforeOperation, BeforeCommit, Committed and Verified. BeforeCommit follows
bounded native staging and precedes the real private commit. Committed follows a
successful commit. No-op migration and identical-policy retries use only
BeforeOperation/Verified, leaving WAL bytes unchanged. Invalid input, conflicts and
old schemas do not synthesize a commit phase. Unguarded calls use the same inner
implementation with no additional guard I/O.

The Root adapter checks original parent selection, full common structure/WALs,
registry directory, retained project key metadata and current service-key validity
at those actual phases and again through the enclosing frame. It derives the
context within that frame and repeats checks before the private operation. It does
not compose an earlier copied file observation with an unguarded account write.

## Refusal, uncertainty and unwind

BeforeCommit refusal drops unconfirmed stages and returns an ordinary typed error.
The original healthy root can be used once an external changed selection is restored.
Native WAL selection also retains its original kernel timestamp. Renaming that
WAL back cannot restore ctime, so an observed native WAL replacement still requires
reopen even before any private write; it remains an ordinary refusal, not a claimed
private commit or whole-root write poison. A no-op's late refusal likewise does not
invent a write. Native private commit errors
retain their typed cause at the AccountStore boundary. A private guard refusal after
successful commit becomes FilePolicyWriteUnknown and withholds the receipt while
preserving committed metadata. Raw AccountStore does not own the external common
namespace and does not pretend to poison that separate Root.

Once Root accepts BeforeCommit, native durable I/O may be attempted. Any later error,
including native commit failure or a final common/key refusal after native success,
returns AccountFilePolicyWriteUnknown with its typed cause and poisons the entire
live root. Existing AccountFileWritePoisoned gates private, SQL, file, snapshot and
key-rotation work. Returning the original external metadata does not resume that
owner. Close it, inspect recovered state and explicitly reopen before deciding what
to do. Never remove confirmed metadata, roll back completed commits or retry blindly.
A successful write leaves the healthy root usable.

Trusted callback panic unwinds normally in the private library: staged transactions
drop and completed commits survive. Root catches the unwind, records whole-root
poisoning after possible publication, then resumes the original panic. A surrounding
catch cannot turn an uncertain original owner into a usable one. This handling does
not introduce panic as a user-input error path; bounded invalid documents return
ordinary typed errors. Process termination is covered separately from unwind.

## Verification boundaries

Native tests cover both WALs, migration/install/replacement, actual original owners
at each phase, current key rotation, cross-project refusal, exact retry/no-op/CAS,
retained-parent movement and independent rebackup/restore. Equal-byte replacements
of common, key or native metadata exercise precommit refusal and postcommit whole-root
poisoning. Trusted unwind and actual signal9 kills check recovered state and a new
independent write. The parent requires signal9 specifically, not merely a failed exit.

Final results are recorded in the [source-bound report](measurements/2026-10-21-file-policy-write-guards/verification.json).
This is native operator metadata administration. Actual installed-policy user-file
execution, HTTP/SDK files, roles, signed URLs, realtime, dashboard and unmet release
acceptance remain open. Synthetic data only; no production-readiness claim.

The subsequent [native user-file read gateway](native-user-file-reads.md) consumes
installed policies for authenticated current-user metadata/list/complete reads under
actual native ownership. Operator metadata methods remain separate. User-file writes
and HTTP/SDK file access are still pending.
