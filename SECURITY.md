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

Private own-WAL accounts implement Argon2id password verification, scoped access
tokens, rotating refresh sessions, revocation and policy-enforced user rows.
Explicit public admission and user-only account/data routes have separate tested
contracts. Native file quotas, joined backup and offline v2 restore are implemented;
they do not grant current user file permissions or enable ordinary v2 startup.

These controls have executed tests; they have not passed an independent security
audit. Role grants/revocation, user-authorized uploads/downloads, signed file URLs,
realtime authorization, supported TLS deployment and secret/backup encryption remain
open. Host-owner/privileged attackers and physical power loss need wider evaluation.
Keep archives private and outside Git; example backup/copy directories are ignored,
but arbitrary operator-selected paths still require deliberate handling. Track evidence
and remaining acceptance in [the audit checklist](docs/security-audit.md) and
[threat model](docs/threat-model.md).
