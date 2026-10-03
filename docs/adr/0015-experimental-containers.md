# ADR 0015: compiled non-root local containers and disposable probes

Status: accepted for the experimental increment.

## Context

The working synchronous Rust engine and Axum server need a reproducible local
deployment without embedding another database service. Packaging must be tested
against actual recovery rather than merely validating YAML. The platform is not
ready for real data or production hosting.

## Decision

Build locked server/CLI release binaries in a multi-stage image with pinned
official Rust/Debian image digests. The runtime contains these binaries,
certificate support and curl for liveness; no Node/Python/database runtime is
required. Debian security package resolution is retained; package-layer bytes
are not claimed reproducible across rebuilds.

Compose uses a private named volume, loopback host port, UID/GID 10001, read-only
root, dropped capabilities, no new privileges, bounded tmpfs and cgroup limits.
It requires a private random master credential rather than a committed default.
Environment delivery is visible to the trusted Docker operator; encrypted
secrets/secret-file delivery remain future work. Rootless local testing uses
cgroup v2 with systemd delegation. Limits are read back inside the real container.

The health check reports liveness only. Preserve operator-visible failures rather
than restart indefinitely. Allow 30 seconds for SIGTERM drain; a forced stop can
leave a durable transaction with no observed response. Same-volume recreation
must preserve exact project keys, transaction state and expected rows.

Implement the orchestration probe in Python's standard library using argument-list
subprocess calls and bounded HTTP. It creates a random isolated Compose project,
synthetic keys and temporary volume, suppresses secret-bearing process/inspection
output, and removes only that project's resources. Actual offline CLI backup,
verification, restore, compaction, forced writer kill and project-journal damage
exercise the image. CI runs the probe plus the project SDK against the container.
Python is a developer/test tool; storage remains the original Rust implementation.

## Consequences and limits

One stopped project database can be backed up/restored independently; archives
do not contain project metadata and are not complete platform backups. Restored
directories are not silently adopted by the registry. Same-revision recreation
and WAL-1/2 behavior are tested; arbitrary upgrade/downgrade compatibility remains
an acceptance gate. Remote deployment, TLS, broad connection/load limits,
browser/dashboard, physical power-loss testing and security audit are pending.
No new database-file format or production readiness claim is introduced.
