# Initial threat model

Assets: stored bytes, project boundaries, credentials, objects and backups.
Inputs: bounded local files, SQL and HTTP now; WebSocket/uploads later.
Trust: the local filesystem and kernel. File locks are advisory; another program
can ignore them. Hardware power-loss guarantees are not assumed from unit tests.

| Threat | Initial mitigation | Remaining work |
| --- | --- | --- |
| Corrupt/truncated file | bounded CRC checks, strict WAL/baseline replay, fail closed | broader faults and failing-media recovery |
| Decoder/query resource exhaustion | bounded files/parser/work, admitted logical projected payload before copying, bounded primary-JOIN retained candidates | model/cache/staging/transient and broader connection/load quotas |
| Concurrent cooperating writers | stable directory owner, staged transactions, competing-process/compaction tests | shared readers, broader network/load quotas |
| Partial writes | committed full-page WAL, ignored partial tail, sync before ACK | actual power loss and broader fault campaigns |
| Accidental overwrite on creation | atomic no-clobber file/directory publication | wider publication I/O failures |
| Malicious local file replacement | private file mode on Unix | trusted directory ownership |
| Project escape / path traversal | server-issued fixed IDs, private directories, scoped capabilities, negative HTTP/path tests | malicious local-owner races, broader audits |
| Injection / privilege escalation | original bounded parser, separate parameters, separate administrator/project scopes | user roles and row policies |
| Secret disclosure | random scoped keys and purpose-bound session tokens, timing-safe digests, bounded zeroizing password owners, static/redacted HTTP logs | environment/transport copies remain, secret encryption and independent audit |
| Offline password guessing | salted fixed-policy Argon2id, admitted zeroizing block workspace, private own-WAL stores and archives | deployment cost/policy calibration, public account policy and independent audit |
| Private account scope confusion | explicit retained root/roster, current service-key recheck, separate project-bound own-WAL stores, epochs and scoped sessions | public account policy, user roles/row policies and whole-process admission |
| Session replay / stale account authority | purpose-bound random access/refresh tokens, single-winner refresh, current epoch/disabled checks, durable logout | external policy, broader load and independent security audit |
| Clock rollback / client deadline injection | trusted system time, durable per-store floor, strict JSON rejects client time, backward time fails closed | trusted OS clock operation and operator recovery procedures |
| Restore revives source user sessions | explicit common-root restore resets each private scope before selection; normal restart preserves scope | service keys remain valid until explicitly rotated; plaintext archive protection |
| Session-family history exhaustion |4096-family cap, explicit current-service-key cleanup1..128, preserve refreshable families, no implicit reset | cleanup appends WAL; offline compaction, scheduling and combined resource budgets remain separate |
| Archive disclosure or substitution | private no-follow single-link registry archives, strict bounds and complete replay | plaintext archives require trusted storage; CRC/SHA do not authenticate malicious rewrites; encryption remains open |
| Accidental live directory replacement | pinned no-follow root/project/data handles, private-mode and device/inode checks before synchronous operations | trusted local operator; no sandbox against malicious privileged namespace mutation |
| Unrecoverable backup | bounded archive, SHA-256/CRC, strict replay, verified staged restore | upgrades, encrypted/incremental backups |

The CLI accepts trusted local paths; it does not provide a sandbox against a
hostile local administrator. Encryption at rest is not implemented. Checksums
must not be described as security signatures. No security or recovery milestone
is complete based solely on the existence of this document.


The private transport currently requires a project service key on every auth
operation, including sign-in and bounded cleanup. It belongs only on a trusted
backend, never in a public browser/mobile client. User tokens still grant no SQL
permission. [The transport contract](private-http.md),
[retained ownership](retained-account-root.md) and
[cleanup decision](adr/0086-bounded-private-session-cleanup.md) describe the tested
boundary. This update records implementation/evidence, not a completed audit.
