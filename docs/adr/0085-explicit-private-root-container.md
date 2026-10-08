# ADR 0085: Separate explicit private-root container configuration

- Status: accepted; hosted container execution is tracked separately
- Date: 2026-10-08

## Context

The native executable now opens an explicitly initialized private root. The
original image selects a legacy registry with EMILYBASE_DATA_DIR. Adding an
account-root variable to that image would correctly refuse both selected modes.
Startup must never initialize, restore, discover or reset private users/sessions.

## Decision

One pinned Dockerfile builds the original Rust binaries and shared unprivileged
runtime. Its accounts target sets only EMILYBASE_ACCOUNT_ROOT. The final registry
target keeps the existing default image behavior and EMILYBASE_DATA_DIR. Neither
target requires a database engine, Node or Python at runtime.

compose.accounts.yaml is an independent configuration with a distinct default
project name and image tag. It selects the accounts target and preserves the
existing loopback port, private named volume, read-only root, UID/GID10001,
capability restrictions, resource limits, bounded temporary directory and manual
restart policy. Do not merge the two Compose files. Explicit -p still selects
the operator's volume identity; changing that identity selects different data.

Initialization and verification are explicit offline CLI operations on a new
target. The first key still requires authenticated operator rotation. Offline
root backup and restore use the existing original-engine publisher. A restored
root is served explicitly with a different root path while the source is stopped;
service keys remain valid but old source user sessions do not authorize the copy.

## Verification boundary

tests/account_containers.py runs a bounded synthetic lifecycle against the actual
compiled server/CLI: initialization, key rotation, user creation, single-winner
refresh, received-ACK SIGKILL, password/disabled epochs, compaction to WAL2,
root backup/verify/no-clobber restore, independently served clone, logout kill,
source authority preservation and corrupt private WAL startup refusal without
repair. Every collected log is screened for generated credentials and identifiers.

Default execution uses real Docker/Compose. It also inspects image mode selection,
non-root execution, directory ownership, read-only root and applied cgroup limits.
The explicitly named --native preflight runs the same lifecycle against local Rust
processes, with five actual SIGKILLs; it makes no container/cgroup claim. A first
preflight exposed the probe's incorrect assumption that method rejection has a
JSON body; the helper now accepts Axum's empty405 response. This was a probe fix,
not a database defect. An unused Python import was removed before final lint.

The GitHub container job executes both legacy and private configurations. Local
Docker is unavailable on this development host: record native checks and hosted
execution separately, and do not mark the latter passed before observing it.

## Consequences and limits

No engine, WAL, root, account, bundle or token format changes. No startup migration
or automatic retry after an unknown outcome. Backups in the source volume cannot
survive volume loss. Trusted operators can inspect environment credentials;
encrypted secrets, secret-file configuration, TLS, public signup, user SQL roles/
row policies, whole-process admission and production acceptance remain open.
