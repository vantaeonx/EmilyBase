# ADR 0012: Private project namespace and scoped high-entropy keys

Status: synchronous registry/key foundation implemented and tested; HTTP pending.

## Decision

Give each project its own managed database under a server-issued random 128-bit
hex ID. Never derive paths from display names, SQL values or unchecked identifiers.
Bound project count and metadata. Use private directory/file modes, reject symlinks,
hold stable root ownership and serialize each project's database requests separately.
The network layer will move synchronous work to bounded blocking workers.

Issue random 256-bit project-owner keys and store SHA-256 digests. Fixed-size secret
comparisons use [subtle's ConstantTimeEq](https://docs.rs/subtle/2.6.1/subtle/trait.ConstantTimeEq.html).
Return plaintext only once to privileged create/rotation callers, with no token logs.
This does not implement user accounts/passwords/sessions; those need a separate design.

Publish a fully initialized project by no-replace staged directory rename and root
sync. Rotate credentials with a synced temporary file, atomic replacement and project
directory sync. Uncertain publication poisons the controller until reopen. Already
accepted requests can finish; new authorization uses the current digest. One-shot
capabilities retain the root inode lock, so dropping a controller cannot release
registry ownership while requests remain. Per-project gates precede database open.

Use a strict, bounded, checksummed JSON metadata envelope with an explicit version,
directory-bound ID and monotonic credential epoch. This changes no database/WAL
format and provides no encryption/authentication against a malicious local owner.
Ignore and preserve unacknowledged creation staging; never silently adopt it.

## Consequences and gates

The library proves logical key scoping and path/metadata bounds with synthetic tests,
including a reproduced controller-drop ownership gap fixed before publication.
It does not yet expose HTTP, administrative authentication, network rate/body limits,
user roles, row policies or platform backups. Those are required before calling
the server stage complete. More publication crash/fault checks and long campaigns
remain open. The private filesystem owner remains trusted.
