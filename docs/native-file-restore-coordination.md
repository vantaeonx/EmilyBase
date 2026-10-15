# Retained native file restore coordination

`with_restored_file_archive_at(bytes, expected_project, parent, leaf, work)` restores
one existing native paired archive beneath an explicitly supplied original parent.
It then calls trusted synchronous work while both original restored writer owners,
the actual child directories, original WAL and every physical object reader remain
held. Orphans are included. The immutable callback report contains copied metadata;
no raw descriptor, mutable database, session or current user authority is exported.

## Private preparation and common selection

The parent must be a real directory with exact0700 permission/special bits. A leaf
is1..255 bytes, without slash, NUL, dot or dot-dot. Verify the complete paired archive
and expected scope before creating any stage. Use the existing descriptor-relative
storage stage, native metadata/object restoration and no-replace child publication.
Moving the original parent does not adopt any replacement at its previous pathname.
Existing files, directories and symbolic links are preserved and never run work.

Before work, compare the actual root/children/WAL identity and bytes, original object
readers, complete inventory, references and persisted quota. Select the complete
child without replacement, sync its parent and repeat those proofs. Work therefore
sees a complete durably selected child, which must still be inside a private enclosing
root if several components are being assembled. The API does not hide an independently
selected child from someone already authorized to inspect its native parent.

Work may nest another checked restoration, then select the enclosing common root
through its original storage stage. All earlier pairs remain locked through that
selection and every unwinding outer proof. After work, repeat the full original-owner
checks before returning its value. Never close and reopen a prepared pair to extend
this boundary: a replacement could otherwise become the newly accepted owner.

## Results and failures

The signature returns `Result<T>` for arbitrary callback value T. A callback's own
`Result` is a value, so callers must flatten both layers. A healthy application refusal
does not delete the already selected child. A later original-owner/privacy/content
failure returns OutcomeUnknown, drops the callback value and preserves the selected
or foreign entries. An enclosing root may already have been selected; its coordinator
must report uncertainty rather than retry, overwrite or claim rollback.

The enclosing coordinator must admit its complete roster, global project/database
bindings, aggregate object/byte/descriptor budgets and trusted reset time before
nesting calls. This low-level function checks one native pair; it deliberately does
not infer account membership or reject a second independently restored copy of the
same scope. Count/report equality is not archive origin authentication.

Trusted callback unwinding releases actual owners through ordinary destructors and
preserves the selected child, but delivers no success receipt. Input processing does
not use panics for refusal. Finite checks are not a namespace lease or protection
against an arbitrary OS owner changing files after return. Existing replay allocations
and up to128 physical descriptors per component remain bounded by their original
contracts, without a whole-process memory/FD reservation or network deadline.

## Compatibility and verification

Ordinary path-based paired restore reuses the retained implementation, preserving
its existing whole-input/no-replace and uncertain-selection behavior. It performs one
additional final proof around its copied report. Original byte formats, session state,
WAL versions and replay/parser semantics stay unchanged. No new parser sanitizer
campaign, common account/file restore, v2 startup, file authorization or production
acceptance follows from this coordination primitive.

[ADR0157](adr/0157-retained-native-file-restore-coordination.md) records the decision.
[Source-bound verification](measurements/2026-10-15-restore-coordination/verification.json)
records the executed final matrix, exclusions and independently checked earlier CI.
Tests cover two nested pairs through enclosing selection, actual writer/payload/WAL
descriptor lifetimes and release,128 physical orphans, callback refusal and unwinding,
moved native parents, invalid leaves/privacy/whole input, existing destinations and
independent source/copy writes. Thirty-six late mutations span both WAL versions,
same-byte inode replacements, corruption, appended tails, links, permissions and
inventory changes; callback receipts are dropped on failure.

Sixteen generated payload/removal models compare exact acknowledged WAL, visible
references, physical charge and original payload bytes. Twelve actual signal9 kills
cover prepared-private, common-selected and returned boundaries for two native WAL
versions and empty/8193-byte payloads. Selected roots reopen independently and permit
the next write; unselected roots remain private. Sources/archives remain byte-exact,
and a fresh independent retry destination can be restored after every kill.

Final affected suites pass813 cases on each Rust1.99/1.89:33 component cases,
12 original file-publication cases,28 registry integration,42 account CLI,
325 original engine,328 storage/object/file and45 native CLI.29 ignored entries
are26 parent-invoked fixtures and three opt-in corpus generators. All649 frozen
source inputs match after every final process closes with0. Formatting, strict
workspace/fuzz lint, affected builds and minimum fuzz compilation pass. Thirteen
new regular cases add768 net Rust lines with tests; total source is138111 lines.

The first fixture refusal is independently reproduced: the temporary parent was
0775 instead of0700. Only fixture permissions change; runtime privacy stays strict.
Failed preliminary runs and poisoned-mutex followers are excluded from final counts.
Earlier hosted172ebaa independently passes1775/0/44 on both workspace toolchains
and all five jobs, with645 committed source hashes checked. Its separate allocation
job passes58 cases, outside those workspace totals. That earlier CI and parser
sanitizer evidence are distinct from this filesystem coordination increment.
