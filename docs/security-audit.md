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

## Required before real deployment

- [ ] Independent review of authentication, project boundaries, logs, parsers
  and publication protocols, with documented findings and repaired regressions.
- [ ] Current dependency advisory and complete transitive-license review.
- [ ] Sustained fuzz campaigns, seed/coverage review and minimized regression
  tests for every finding, beyond the current bounded smoke runs.
- [ ] Wider failing-media and actual power-loss matrix; corruption across every
  supported persisted format and durable upgrade/restore compatibility.
- [ ] Load, slow-client, cancellation and connection-exhaustion tests. Current
  request limits do not claim a complete connection admission controller.
- [ ] Supported TLS/reverse-proxy deployment, protected master-secret handling,
  encrypted-secret design and incident/rotation procedures.
- [ ] Password hashing, user sessions/refresh rotation, roles and row policies
  before presenting these as available platform features.
- [ ] Private object storage, signed URL validation and realtime authorization
  before enabling those future interfaces.

See [executed tests](testing.md), [roadmap](roadmap.md) and
[file-format compatibility](file-format.md). No milestone is closed by this list.
