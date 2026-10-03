# Initial threat model

Assets: stored bytes, project boundaries, credentials, objects and backups.
Inputs: bounded local files, SQL and HTTP now; WebSocket/uploads later.
Trust: the local filesystem and kernel. File locks are advisory; another program
can ignore them. Hardware power-loss guarantees are not assumed from unit tests.

| Threat | Initial mitigation | Remaining work |
| --- | --- | --- |
| Corrupt/truncated file | bounded CRC checks, strict WAL/baseline replay, fail closed | broader faults and failing-media recovery |
| Decoder resource exhaustion | page and file bounds | broader connection/load quotas |
| Concurrent cooperating writers | stable directory owner, staged transactions, competing-process/compaction tests | shared readers, broader network/load quotas |
| Partial writes | committed full-page WAL, ignored partial tail, sync before ACK | actual power loss and broader fault campaigns |
| Accidental overwrite on creation | atomic no-clobber file/directory publication | wider publication I/O failures |
| Malicious local file replacement | private file mode on Unix | trusted directory ownership |
| Project escape / path traversal | server-issued fixed IDs, private directories, scoped capabilities, negative HTTP/path tests | malicious local-owner races, broader audits |
| Injection / privilege escalation | original bounded parser, separate parameters, separate administrator/project scopes | user roles and row policies |
| Secret disclosure | random scoped keys, SHA-256 digests, fixed-size timing-safe checks, verified log redaction with static method labels | passwords/sessions, secret encryption and wider audit |
| Archive disclosure or substitution | private no-follow single-link registry archives, strict bounds and complete replay | plaintext archives require trusted storage; CRC/SHA do not authenticate malicious rewrites; encryption remains open |
| Unrecoverable backup | bounded archive, SHA-256/CRC, strict replay, verified staged restore | upgrades, encrypted/incremental backups |

The CLI accepts trusted local paths; it does not provide a sandbox against a
hostile local administrator. Encryption at rest is not implemented. Checksums
must not be described as security signatures. No security or recovery milestone
is complete based solely on the existence of this document.
