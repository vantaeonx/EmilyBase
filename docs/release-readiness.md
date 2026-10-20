# Release scope and evidence

EmilyBase remains experimental and uses synthetic data only. This document is a
current scope index and a release preparation gate, not release notes or approval
for production. A crate version or successful build alone is not a published release.

## Current scope

| Area | Implemented boundary | Remaining release scope |
| --- | --- | --- |
| Storage | Original synchronous Rust pages, checksums, typed tables and CRUD | Wider corruption/failing-media and upgrade compatibility matrix |
| Durability | Original WAL1/2, atomic managed transactions, strict replay, checkpoint and explicit compaction | Actual power-loss evidence, wider fault campaigns and stable supported-format policy |
| Indexes | Original B+ tree and independently checked derived/standalone physical index work | Combined durable table-index writer and secondary-index DDL |
| SQL | Original lexer, parser, AST, planner and bounded documented executor | Wider syntax/semantics; no PostgreSQL compatibility promise |
| Server | Axum/Tokio adapter over synchronous owned engine, isolated projects, service keys and typed APIs | Broader load/connection/output admission and deployment acceptance |
| Accounts | Original private account/session stores, password hashing, rotation, epoch revocation and admitted sign-in | Public signup, roles, broader account policy and independent review |
| Rows | Current installed-policy enforcement, typed CRUD and filtered user pages | Broader policy/role orchestration and independent side-channel review |
| Files | Native objects/archives; own-WAL references, CAS/quota, common native root creation, paired backup/restore CLI and bounded account/file component capture | Common runtime adoption, user file HTTP/policies, reclamation and signed URLs |
| Backup | Verified database/registry/private-root copies; native paired and joined archives, coordinated backup and offline v2 restore CLI | Explicit live v2 adoption, encrypted/incremental backup and upgrade drills |
| SDK | Experimental TypeScript service client and explicit user client | Kotlin SDK, packaging and broader compatibility coverage |
| Realtime | No released committed-change subscription service | Journal-to-subscription integration, resumption and authorization |
| Dashboard | No released administrative web interface | React/TypeScript interface backed by real protected APIs |
| Operations | Docker/Compose and synthetic container probes | External deployment, sustained load, secret encryption and independent security audit |

Rust implements storage, SQL, server and CLI. TypeScript is a client SDK; it does not
replace the original engine. Source availability does not publish hosted user data.
Existing v1 account-root bundles exclude native object directories. The separate
joined format carries paired file archives and supports native private file
publication. Common root restore and runtime adoption remain open.
The [AccountRoot object integration proposal](adr/0127-proposed-account-root-object-integration.md)
keeps the exact schema, ownership, quota and coordinated-restore gates explicit;
its proposed status must not be described as a released upload feature.

The [roadmap](roadmap.md) owns stage acceptance. [Testing](testing.md) records
executed checks and their exact boundaries. [Security acceptance](security-audit.md)
and [recovery matrix](recovery-matrix.md) keep broader gates open. Historical ADRs
describe their own increments; this table reflects later implemented additions.
The complete [d988a51 integration snapshot](measurements/2026-10-10-integration-checks/verification.json)
records complete1518-test stable/minimum matrices, builds, parser/SDK/audit checks
and successful source CI, while preserving earlier snapshot boundaries. It is not
a release acceptance certificate.
The later immutable [24444bc full integration snapshot](measurements/2026-10-10-complete-reader-publication/verification.json)
passed1552 workspace tests and full builds on stable1.99 and minimum1.89. All580
frozen hashes match;32 default ignored entries comprise30 parent-invoked fixtures
and two separate opt-in corpus generators. The interrupted minimum attempt is
excluded; the resumed complete command exited0. Its hosted source run also passed.
This completed snapshot does not certify later selected readers or file catalogs.
The later [native inspection increment](native-object-inspection.md) has its own
172-check targeted matrices; it does not claim a new complete broad result.
The following [seekable archive increment](seekable-object-archive-inspection.md)
has181-check targeted matrices and its own parser comparison; the same broad-result
boundary applies until a new full source run completes.
The later [hosted9ad9194 full-source evidence](measurements/2026-10-15-hosted-file-root/verification.json)
was retrieved and checked on2026-10-15:1648 workspace tests per Rust1.99/1.89,
39 ignored entries (35 parent-invoked fixtures/four opt-in corpus generators), and
614 committed source blobs matching the feature evidence. Its stable build, SDK,
fuzz, opt-in diagnostics, audit and container jobs completed successfully on2026-10-10.
This is hosted evidence for9ad9194; it is neither a new local container run nor full
certification of subsequent diagnostic changes. An earlier local dc86d4c minimum
run has no recovered final result after restart and is excluded.
The [borrowed encoder](borrowed-object-archive-encoding.md) follows with188-check
targeted matrices and an expanded actual encoded-reader sanitizer comparison.
Native backup publication is unchanged in that increment; it closes no release gate.
The subsequent [reader publisher](reader-file-publication.md) connects native backup
with199-check targeted matrices, five new process-kill boundaries and actual maximum
independent restore. Its evidence records the reproduced/fixed test interference;
it does not close the broad resource, power-loss or production gates.
The [selected native retention increment](selected-object-retention.md) has209-check
targeted matrices, explicit descriptor/native mutation coverage and two owner-lifetime
compile-fail checks. It supplies no catalog transaction or current user authority.
The [Pager owner correction](pager-owner-lock-lifetime.md) follows with a reproduced
Busy regression and534-check affected matrices on both toolchains. Its separate
WAL ownership contract remains unchanged; this is not a complete current workspace run.
The [native payload reader](object-payload-reader.md) adds bounded payload-only
reads retaining the original owner/inode, irreversible refusal after detected
change, checked seeks and final complete hashing. Its targeted evidence is separate
from the complete older source and grants no user file service or release approval.
The [standalone native file catalog](native-file-catalog.md) adds actual own-WAL
references and persisted physical quotas with invisible charged orphans. Its native
process-kill/reopen/semantic-corruption evidence does not implement coordinated
Root backup, current file policies or user endpoints.
Native [metadata mutations](native-file-mutations.md) add reference-revision CAS,
logical removal and native WAL1/2 kill/reopen evidence. Physical blobs remain charged
after removal; user authorization and common restore are still pending.
Native [quota administration](native-file-quota-administration.md) adds scoped
global-revision changes under retained complete physical inventory. Its bounded
native descriptors are not global server FD admission or current user authority.
Native [coordinated immutable capture](native-file-snapshot.md) pairs verified
metadata backup and complete object images under their original owners. Component
round trips preserve the graph; common durable publication/restore remains pending.
The [experimental paired archive format](native-file-archive-format.md) validates
both components, exact scope, physical quota and reference hashes together. Its
borrowed encoder/byte decoder do not close common publication or restore gates.
Native [paired archive publication](native-file-archive-publication.md) adds
standalone no-replace common backup files and readonly inspection with actual
descriptor retention. Coordinated AccountRoot inclusion remains open, separately
from process-kill publication checks.
Native [common paired restore](native-file-archive-restore.md) verifies and stages
both components under one selected root, retaining actual child/WAL/object owners
through final checks. AccountRoot coordinated inclusion, current user policy,
power-loss evidence and production gates remain open.
The [native paired archive CLI](native-file-archive-cli.md) binds file restoration
to the original readonly input/parent through selection and handles failed report
delivery without overwriting the selected root. It grants no user authority.
Native [fresh file-root creation](native-file-root-initialization.md) now uses the
original engine and marker publisher, retained actual children and common selection
with explicit physical quotas. It does not add current user/AccountRoot file routes.
Native [common-root observation](native-file-root-observation.md) retains the selected
root, actual children/WAL/scope and both component owners around metadata/capture.
It preserves abandoned tails and adds no common-root write or user-file authority.
The following [guarded native publication](native-file-root-publication.md) adds
blob-first writes with fallible checks at original commit boundaries and same-inode
acknowledged WAL readback. Native [guarded metadata mutations](native-file-root-metadata.md)
add original reference/quota transactions with exact scoped revision checks and
physical orphan charging. AccountRoot inclusion and current user routes remain open.
Native [retained payload cursors](native-file-root-stream.md) add bounded logical
reference reads and raw CLI output under original common owners. Their final
verification does not imply atomic delivery or current user download authority.
The [future v2 file-root roster codec](account-file-root-manifest.md) is pure metadata
inspection with explicit compatibility refusal. Ordinary account-root operation and
combined backup/restore have not adopted it; those runtime gates stay open.
Native [parent-relative account opening](parent-relative-account-root.md) preserves
existing v1 owner/selection lifetimes for future common ownership. It does not adopt
the new manifest or certify coordinated files/users backup or permission.
Native [live account-root capture](live-account-root-capture.md) now retains every
original public/private owner through a checked internal coordination callback and
returns copied sensitive v1 bytes. Actual file capture in that boundary, common
publication/restore and current user file authority remain release gates.
Native [parent-relative file components](parent-relative-file-root.md) likewise
preserve original metadata/object owners across parent release/moves and check
supplied root selection without rebinding. This prerequisite adopts no joined
account/file runtime or backup format and closes no user-file/release gate.
Native [snapshot coordination](native-file-snapshot-coordination.md) now retains
every original payload through nested callback work and final proofs. It closes
a component lifetime prerequisite, while aggregate admission and the actual joined
account/file collector/format/restore remain open.
The [native component collector](account-file-component-capture.md) now closes that
explicit capture step with original nested owners and aggregate physical-object/
copied-image bounds. Caller-selected files must belong to declared private projects;
database identities must remain globally distinct. The copied v2 declaration is
not installed as a live root. Joined archive/publication/restore, current user file
permissions and full memory/worker reservations remain release gates.
The [joined byte format](account-file-archive-format.md) subsequently adds a
bounded canonical envelope and borrowed encoder over those immutable copies. It
verifies exact nested scope/roster/identity and aggregate limits. Actual common
backup publication/restore, filesystem adoption and user routes remain open.
The later [private joined publisher](account-file-archive-publication.md) now adds
original-inode no-replace file publication, exact readback and explicit uncertain
late results, plus readonly native/CLI verification. A published archive file does
not close common root restore, layout adoption or current user-file authorization.

## Before publishing an experimental release

- [ ] Select an exact version, source commit and immutable tag; verify the public
  release refers to that commit and is explicitly marked experimental.
- [ ] Run formatting, strict lint, workspace tests/build, minimum Rust compatibility,
  SDK unit/live tests and applicable container checks on that exact source. Record
  commands, toolchains, exit status and complete source hashes.
- [ ] Require the new source's hosted CI result. A prior green revision is not
  evidence for a later source; external setup failures need a successful fresh run.
- [ ] Confirm acknowledged/unacknowledged recovery, corruption, concurrent writers
  and full verified backup/restore for the actual supported runtime modes. Preserve
  source scopes and validate independent writes after restore.
- [ ] Review documented file versions, bounds, supported upgrade paths and rollback.
  Refuse unsupported inputs rather than treating a new format as compatible.
- [ ] Refresh dependency/license/secret review and record open security findings.
  No signing secret, private database or account data belongs in release assets.
- [ ] Execute documented start, synthetic CRUD, restart, backup and restore examples
  from a clean checkout. Describe missing services and operator-only primitives.
- [ ] Publish bounded reproducible demonstrations and evidence links for this
  version. Measure performance only with a stated workload and methodology.
- [ ] Prepare release notes around actual behavior, known limitations and recovery
  instructions. An initial experimental release need not claim all roadmap stages.

Production qualification requires the separate open recovery, backup, load, upgrade
and independent security gates. Passing this preparation list cannot close them
implicitly. Actual power loss is distinct from killing a process.

## Public case-study facts

A case study can present original engine ownership, architecture, verified release
behavior and selected engineering decisions with source/evidence links. It should
name the precise version and distinguish native tools from user-facing services.
Do not advertise a planned dashboard, realtime or Kotlin SDK as released. Treat
source line count as a secondary measurement, not a readiness or quality percentage.

The [retained private restore adapter](private-account-restore-coordination.md) now
prepares copied account sessions under the original database owner. Its borrowed
source proof catches an observed pre-reset tail before ordinary WAL trimming.
This closes that component prerequisite; complete account/file root restoration,
v2 runtime adoption and the release acceptance boxes above remain open.

[Explicit offline joined restoration](account-file-root-restore.md) subsequently
closes the library-level common restore prerequisite. Original readers and a common
lock span the last proof after component writers unwind. The restored v2 layout
still requires future explicit live adoption; joined CLI and the acceptance boxes
above remain open.

The [joined restore CLI](account-file-restore-cli.md) subsequently supplies a path
from a retained private archive to an offline v2 copy, with source acceptance inside
the final common lock. It emits bounded counts only and preserves copies on output
failure. Explicit v2 live adoption and the release acceptance boxes remain open.

The [coordinated backup CLI](account-file-backup-cli.md) subsequently completes the
native operator create/verify/restore path. Original source capture spans publication;
selected archive identity spans final source unwind. Ancestry preflight refuses output
inside original source roots before staging. File scopes remain explicit and bounded;
v2 live adoption and all unfulfilled release acceptance boxes remain open.

The [explicit v2 library opener](explicit-v2-account-root.md) subsequently admits
retained native file owners with exact scope/identity checks and preserves normal
account/data operations across reopen. Actual TCP exercises explicitly constructed
library routing; default server-binary startup remains v1. Complete v2 rebackup and
explicit binary configuration remain open; old capture paths refuse v2 rather than
silently drop its file bindings. The release acceptance boxes remain open.

The subsequent [complete v2 backup](v2-root-backup.md) supplies readonly capture
and publication using internally retained native components, preserves every binding
and supports a second independent restore. Joined v1/v2 capture checks the physical
common tree after reproduced private/public unacknowledged-tail regressions. This closes
the native v2 rebackup prerequisite, not binary deployment or any unfulfilled
production acceptance box above.

The [explicit binary v2 mode](v2-server-startup.md) subsequently closes native
server configuration and actual binary HTTP/restart prerequisites. Both WALs,
acknowledged mutation, actual kills, incomplete request bodies, session preservation
and independent rebackup/restore are exercised. Docker is unavailable locally;
Compose selection is not a claim of completed container v2 recovery/upgrade.
All unfulfilled production acceptance boxes remain open.

The [native project-service read gateway](project-file-read-gateway.md) subsequently
allows original internally owned file observations and complete copied payloads
under current service keys. It adds per-call original project metadata selection
after a reproduced equal-byte replacement failure. This supplies readonly native
integration, not user file policy, a mutation endpoint or a production gate.

The subsequent [native write guards](native-file-write-guards.md) supply trusted
coordination at actual blob/WAL boundaries while preserving original owners,
readers, staged refusal and uncertainty. Both WALs and 54 actual signal9 kills per
toolchain exercise recovered state. This closes a native coordination prerequisite;
current-key/common-root mutation integration and unfulfilled production boxes remain open.

The subsequent [native current-key file mutations](project-file-write-gateway.md)
connect actual write-phase guards to the owned common root and retained project
metadata. Late uncertainty closes the whole live root until explicit reopen.
This closes native backend mutation integration, not user-file permissions,
HTTP/SDK mutation or any unfulfilled production acceptance box.

The subsequent [bounded file-policy decision library](file-policy-decisions.md)
supplies strict per-action owner/session rules over genuine borrowed principals
and exact native scope. Its parser/model checks are a prerequisite. Installed
private policy authority, owned native user-file enforcement, HTTP/SDK file access
and all unfulfilled production acceptance boxes remain open.
