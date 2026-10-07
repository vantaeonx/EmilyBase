# ADR0076: owned private account-bundle files and count-only CLI inspection

Status: accepted for local file publication/inspection; combined restore open.

## Decision

An in-memory common capture is useful only when it can be safely persisted and
verified. Reuse the tested descriptor-owned registry file publisher for both
registry and account-bundle envelopes, preserving legacy format/report behavior.
The original engine, live ownership boundary and EMILYBND-1 bytes do not change.

ProjectStore::backup_account_bundle first captures an explicit private roster at
the common boundary, then publishes those immutable sensitive bytes outside the
registry. It never replaces an existing destination. Data owners can be released
when capture returns: subsequent source commits do not alter the captured image.
Caller AccountStores remain exclusively owned. Paths are trusted offline operator
inputs; choose a separate private backup directory. No account roster is discovered.

A shared typed publisher selects format parser, byte cap, private temporary prefix
and distinct sync/kill points. It validates input before staging, writes a0600
private file under a pinned real parent, fsyncs it, reads through its descriptor,
checks exact bytes and complete nested semantics, and publishes with Linux
no-replace rename. Owned-parent fsync and final parent/selected-inode checks precede
success. Before-publication errors remove only still-owned temporary entries;
substitutions/detached originals are preserved. Errors after rename report unknown
publication durability/selection and preserve the selected state for inspection.
The legacy EMILYREG publisher retains its old prefix and test boundaries.

The file reader refuses final symlinks, nonregular files, multiple hard links,
broad permissions and bounded-size violations before reading payloads. NONBLOCK
ensures a FIFO cannot turn metadata refusal into a hanging open. Full-file reads
and private replay remain bounded by formats; streaming, encryption and numeric
whole-process model/transient/output reservations are not implemented.

Expose inspect_account_bundle for trusted private local files. Add the real
account-bundle-verify CLI command, printing aggregate counts only: projects,
private stores, data tables/rows, accounts, retained families and archive bytes.
It never prints project IDs/names, data rows, private logins, passwords, verifiers
or tokens. Inspection does not open a live database, reset a scope or change
credentials. The command verifies a file; it does not create or restore a bundle.

## Compatibility and evidence

EMILYBND/EMILYREG/EMILYBAK and embedded database/WAL versions remain unchanged.
No new runtime library or finished database engine is used. The own auth library
is added only as a CLI test dependency. Existing registry publication/restore,
backup and HTTP/SDK behavior are covered by the full regression suite.

Twelve new server cases check exact private export and report/readback, no replace,
private modes, source ownership, malformed bytes before staging, nonregular/alias/
permission/size rejection, final-parent symlinks, changed staged bytes/modes and
parent/staging/selected substitutions. Eight injected failures before/after actual
file/parent fsync cover both WAL versions and distinguish no selection from
post-rename uncertainty. A24-case independent model verifies rows, private count,
source immutability, safe retry and refusal to overwrite the first selected image.

Twelve forced-process-kill cases cover common data/private capture, synced file,
rename, parent sync and returned success across both WAL versions. Partial or
uncommitted temporary files never become selected archives; complete selected
images replay and source histories remain exact. A synchronized two-process file
publication race selects exactly one complete image. Private killed-process
remnants may remain; no automatic sweeper or staging adoption is introduced.

Three native CLI cases cover all private versions1/2/3 and both WAL versions,
relative Unicode paths, empty/subset rosters, source/archive immutability and
continued password/session/API-key validity. Unsafe/corrupt inputs fail with no
private output or newly created paths. The prior parser ASan campaign remains
recorded separately; this publication/CLI change makes no new parser campaign or
physical power-loss claim. Actual final checks are in testing.md.

## Next gate

Restore every selected data/private store into one pinned root, validate and
reset every private session scope before root publication and traffic. Generic
engine/registry restore preserves old private scope/key metadata and cannot be
substituted for that gate. An authoritative private roster, HTTP users/workers,
roles/RLS, numeric resource reservations and broader production security/load/
upgrade/crash acceptance remain open.
