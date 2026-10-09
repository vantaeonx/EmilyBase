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
| Files | Native scoped immutable objects, streamed inventory/inspection, bounded writes, standalone backup and verified restore | AccountRoot coordination, user file HTTP/policies, persisted quotas and signed URLs |
| Backup | Verified native database/registry/private-root copies; standalone object archives | Coordinated inclusion of objects, encrypted/incremental backup and upgrade drills |
| SDK | Experimental TypeScript service client and explicit user client | Kotlin SDK, packaging and broader compatibility coverage |
| Realtime | No released committed-change subscription service | Journal-to-subscription integration, resumption and authorization |
| Dashboard | No released administrative web interface | React/TypeScript interface backed by real protected APIs |
| Operations | Docker/Compose and synthetic container probes | External deployment, sustained load, secret encryption and independent security audit |

Rust implements storage, SQL, server and CLI. TypeScript is a client SDK; it does not
replace the original engine. Source availability does not publish hosted user data.
Current root bundles exclude the separate native object directories.
The [AccountRoot object integration proposal](adr/0127-proposed-account-root-object-integration.md)
keeps the exact schema, ownership, quota and coordinated-restore gates explicit;
its proposed status must not be described as a released upload feature.

The [roadmap](roadmap.md) owns stage acceptance. [Testing](testing.md) records
executed checks and their exact boundaries. [Security acceptance](security-audit.md)
and [recovery matrix](recovery-matrix.md) keep broader gates open. Historical ADRs
describe their own increments; this table reflects later implemented additions.
The [integration evidence snapshot](measurements/2026-10-10-integration-checks/verification.json)
separates completed parser/SDK/audit checks, earlier green complete baselines and
the pending full matrix. It is not a release acceptance certificate.

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
