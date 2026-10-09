# Security acceptance checklist

This is an evidence register for an experimental build, not an audit certificate.
Checked entries mean the named local tests ran, not that a control is complete
against every attacker. Real application data remains prohibited pending the
recovery, backup and security acceptance gates.

## Implemented controls with executed evidence

- [x] Original synchronous engine, without a ready database engine dependency.
  Locked workspace metadata inspected on 2026-10-03: 11 Apache-2.0 workspace
  crates; no SQLite/PostgreSQL/MySQL/DuckDB/MongoDB/RocksDB/sled/redb/LevelDB
  dependency names. This name inspection is not a dependency vulnerability audit.
- [x] Workspace source forbids unsafe code. Fuzz targets do likewise. This does
  not forbid unsafe implementation inside ordinary third-party dependencies.
- [x] Both locked dependency graphs checked with cargo-audit 0.22.2 on
  2026-10-04, without ignored advisories/target filters and with warnings denied.
  RustSec database revision ef6173cbc5c50ec8166f9a5b28f07834144373ee contains
  1290 advisories, updated 2026-10-03. The workspace and fuzz graphs report zero
  known vulnerabilities and no warnings. CI repeats these checks on changes.
  This checks known entries in the [RustSec database](https://rustsec.org/), not
  the original engine, binary contents or unknown supply-chain vulnerabilities.
  Rechecked both lockfiles after adding password cryptography on 2026-10-07:
  cargo-audit 0.22.2, RustSec b8a1a33e246a0a9a3b5f377248c41a503defec74,
  1294 advisories, 163 workspace/134 fuzz packages, warnings denied and no ignored
  entries. Both checks exit successfully with no reported vulnerabilities/warnings.
- [x] Bounded checksummed page/WAL/index decoders, explicit format versions and
  typed rejection. Raw and checksum-repaired ASan targets have executed; short
  runs do not establish exhaustive coverage.
- [x] SQL binding, typed parameters, token/depth/script/work/result bounds,
  three-valued filters and validation on empty inputs. Original parser/executor
  tests, generated models and ASan campaigns execute.
- [x] Durable acknowledgment, rollback, strict replay, checkpoint, compaction and
  corruption rejection. Kill/fault/concurrent-process tests execute for supported
  WAL versions; physical power loss is a separate pending gate.
- [x] API-key digests, fixed-size timing-safe comparisons, scoped authorization,
  master/project separation and rotation. Real HTTP negative tests execute.
- [x] Descriptor-bound project directories, private metadata and no-clobber
  publication. Symlink/hardlink/FIFO/traversal and directory-replacement tests
  execute, including accepted requests interrupted around path replacement.
- [x] Verified single-database and whole-registry backups/restores, private
  destinations and independent post-restore writes. Actual restored HTTP and
  Docker restart/kill campaigns execute with synthetic data.
- [x] Optional cache identity/history/live-pointer validation and bounded startup
  adoption after mandatory WAL replay. Stale/corrupt/foreign caches cannot supply
  missing acknowledged history or prevent tested fallback to live rows.
- [x] Static request-log fields and private-error redaction. Real native/container
  cases check keys, IDs, SQL and extension-method text do not appear in logs.
- [x] Private non-root container, read-only runtime root, dropped capabilities,
  process/memory/CPU bounds and loopback publication verified by the container probe.
- [x] Tracked artifact names inspected on 2026-10-03: no .env, database/cache,
  signing-key or private-key filenames. This is not an exhaustive secret scan.

- [x] Current installed-policy CRUD and filtered pages under simultaneous public/
  private ownership, with strict two-credential trusted backend HTTP, current key/
  policy/session body-wait checks, duplicate/size/typed rejection, bounded complete
  responses and static secret-free logs. Source-bound stable/minimum, generated,
  real TCP received/unread-result recovery, verified clone and sanitizer evidence
  are recorded in [ADR0106](adr/0106-trusted-backend-user-row-http.md).

## Required before real deployment

- [ ] Independent review of public user-only admission, role grants/revocation,
  policy timing/constraint side channels and whole-process/connection/output
  admission. Service operators retain trusted SQL/row authority; the backend
  adapter is not a sandbox against them. No production audit is declared complete.



- [ ] Independent review of authentication, project boundaries, logs, parsers
  and publication protocols, with documented findings and repaired regressions.
- [ ] Complete transitive-license/source review and independent supply-chain
  audit; ongoing known-advisory checks do not establish these controls.
- [ ] Sustained fuzz campaigns, seed/coverage review and minimized regression
  tests for every finding, beyond the current bounded smoke runs.
- [ ] Wider failing-media and actual power-loss matrix; corruption across every
  supported persisted format and durable upgrade/restore compatibility.
- [ ] Load, slow-client, cancellation and connection-exhaustion tests. Current
  request limits do not claim a complete connection admission controller.
- [ ] Supported TLS/reverse-proxy deployment, protected master-secret handling,
  encrypted-secret design and incident/rotation procedures.
- [ ] Public account policy, role grants/revocation, enumeration/load controls and
  independent integrated security review before public user-only admission.
  Private account storage/session rotation, retained owners, current-key HTTP
  admission, owned policy CRUD/filtered pages and coordinated root restore have
  separate executed evidence in [ADR0068](adr/0068-private-project-account-store.md),
  [ADR0078](adr/0078-atomic-account-bundle-root-restore.md),
  [ADR0080](adr/0080-retained-private-root-service.md) and
  [ADR0106](adr/0106-trusted-backend-user-row-http.md). Dynamic private roster and
  the broader production gates remain open.
- [ ] Private object storage, signed URL validation and realtime authorization
  before enabling those future interfaces.

See [executed tests](testing.md), [roadmap](roadmap.md) and
[file-format compatibility](file-format.md). No milestone is closed by this list.


## Dependency refresh, 2026-10-10

On source479d0a2, cargo-audit0.22.2 checks both locked graphs with warnings denied
and no ignored entries. RustSec7eebec69c352c7191b1f13eb95dd510eeca5d1de contains
1296 advisories, updated2026-10-09T10:12:02+02:00. Workspace166/fuzz137 packages
report zero known vulnerabilities and no warnings; the second check uses the same
cached database with no fetch. Current streaming changes leave both lockfiles
unchanged. All17 own workspace packages declare Apache-2.0/edition2024/minimum1.89.
An explicit dependency-name inspection finds none of the listed ready engine/SQL
parser packages; this is not a complete source or transitive-license audit.
