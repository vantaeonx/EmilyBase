# Initial threat model

Assets: stored bytes, future project boundaries, credentials, objects and backups.
Inputs: malformed local files now; SQL, HTTP, WebSocket and uploads later.
Trust: the local filesystem and kernel. File locks are advisory; another program
can ignore them. Hardware power-loss guarantees are not assumed from unit tests.

| Threat | Initial mitigation | Remaining work |
| --- | --- | --- |
| Corrupt/truncated file | fixed sizes, checksums, structural checks | recovery from WAL |
| Decoder resource exhaustion | page and file bounds | query/network quotas |
| Concurrent cooperating writers | exclusive file lock | transaction coordinator |
| Partial writes | sync and detect corruption | WAL and torn-write repair |
| Accidental overwrite on creation | no-clobber publication | crash matrix |
| Malicious local file replacement | private file mode on Unix | trusted directory ownership |
| Project escape / path traversal | no network or public path API yet | project-bound path handles |
| Injection / privilege escalation | no SQL/API yet | parser, policy tests, authorization |
| Secret disclosure | no secrets required or committed | hashing, constant-time checks, redaction |
| Unrecoverable backup | no backup feature yet | manifest checks and verified restore |

The CLI accepts trusted local paths; it does not provide a sandbox against a
hostile local administrator. Encryption at rest is not implemented. Checksums
must not be described as security signatures. No security or recovery milestone
is complete based solely on the existence of this document.
