# ADR0163: publish joined backups inside original component capture

Status: accepted for experimental trusted native Linux backup and CLI.

## Decision

Expose backup_account_file_components over actual AccountRoot/NativeFileRoot owners.
Run the existing streamed checked publisher inside the innermost component callback.
Refactor only private publication internals to retain the actual selected file and
baseline until the outer source proofs and final destination check complete. The
immutable snapshot publisher still returns the same copied report and contract.

Preflight and repeatedly check native destination ancestry against every actual
source root. A bounded storage guard independently opens readonly directory
descriptions, compares dev/inode and walks at most256 ancestors. It prolongs no
source flock and exports no source descriptor. Reuse one original private target
context; do not reopen/adopt a foreign former pathname or merely reject a newly
created source-contained archive after publication has already changed inventory.

Expose account-file-backup ROOT ARCHIVE with optional file parent and an explicit
unique canonical project list (at most four). Validate list shape before acquiring
owners and declared private membership before opening any file roots. Never infer
scope from arbitrary directories, create absent stores or print sensitive content.
Reuse the existing nine-field inspector report, preserving exact decimal time.

## Consequences and acceptance

Selected artifacts survive late failure with ComponentPublicationUnknown; source
unwind cannot turn a publisher uncertainty into an ordinary prepublication error.
Failed stdout can follow completion and requires inspection rather than blind retry.
Source sessions, scopes and clocks are unchanged. Payload/public writer lifetimes
end at their respective capture proofs; the selected archive spans their unwind.
This is finite native observation, not a namespace lease, encrypted/provenanced
backup, current user file permission or whole-process reservation.

Test actual original owners/readers, contained targets before staging, late source
and post-capture archive changes, sync failures, original parent movement, generated
explicit subsets, dedicated signal9 workers and real CLI backup/verify/restore.
Document and test the true writer/reader lifetimes without weakening ownership.
No format/dependency-version change or ordinary v2 startup is accepted here. Live
v2 adoption must preserve explicit file bindings and receive separate acceptance.
See the [operator contract](../account-file-backup-cli.md).
