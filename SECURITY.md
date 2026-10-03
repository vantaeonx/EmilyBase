# Security

EmilyBase is experimental and has no supported production release.
Use synthetic data only. No security audit has been completed.

Report a vulnerability through the repository's private vulnerability reporting
feature when available. Otherwise contact the maintainer through their GitHub
profile to arrange private disclosure. Never publish credentials or private data
in issues. Include the commit, operating system, reproduction and impact.

Current implemented scope includes bounded file/WAL/index/SQL decoding,
corruption detection, verified recovery and backups, scoped HTTP API keys,
key rotation, project isolation, request limits and secret-safe logs. Private
directory descriptors bind accepted scoped operations to their original objects.
Optional table caches never replace mandatory WAL verification.

These controls have executed tests; they have not passed an independent security
audit. User passwords, refresh sessions, roles, row policies, file URLs, TLS
deployment and secret encryption are not implemented. Host-owner/privileged
attackers and physical power loss need wider evaluation. Track concrete evidence
and remaining acceptance in [the audit checklist](docs/security-audit.md) and
[threat model](docs/threat-model.md).
