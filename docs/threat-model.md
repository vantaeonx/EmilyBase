# Initial threat model

Assets: stored bytes, future project boundaries, credentials, objects and backups.
Inputs: malformed local files now; SQL, HTTP, WebSocket and uploads later.
Trust: the local filesystem and kernel. File locks are advisory; another program
can ignore them. Hardware power-loss guarantees are not assumed from unit tests.

| Threat | Initial mitigation | Remaining work |
| --- | --- | --- |
| Corrupt/truncated file | bounded CRC checks, strict WAL replay, fail closed | rotation and failing-media recovery |
| Decoder resource exhaustion | page and file bounds | query/network quotas |
| Concurrent cooperating writers | exclusive owner, staged transactions, competing-process tests | shared readers, server quotas |
| Partial writes | committed full-page WAL, ignored partial tail, sync before ACK | actual power loss and broader fault campaigns |
| Accidental overwrite on creation | atomic no-clobber file/directory publication | wider publication I/O failures |
| Malicious local file replacement | private file mode on Unix | trusted directory ownership |
| Project escape / path traversal | no network or public path API yet | project-bound path handles |
| Injection / privilege escalation | no SQL/API yet | parser, policy tests, authorization |
| Secret disclosure | no secrets required or committed | hashing, constant-time checks, redaction |
| Unrecoverable backup | bounded archive, SHA-256/CRC, strict replay, verified staged restore | upgrades, encrypted/incremental backups |

The CLI accepts trusted local paths; it does not provide a sandbox against a
hostile local administrator. Encryption at rest is not implemented. Checksums
must not be described as security signatures. No security or recovery milestone
is complete based solely on the existence of this document.
