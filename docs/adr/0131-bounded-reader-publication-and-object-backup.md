# ADR0131: bounded immutable reader publication for native object backup

Status: accepted for experimental synthetic-data use.

## Context

ADR0130 provides canonical immutable archive bytes without duplicating payloads.
The existing byte-slice publisher requires an owned output image. Removing full
byte readback or changing publication ownership to save that image would weaken
the native reliability contract.

## Decision

Add a separate synchronous descriptor-relative publisher for exact bounded
immutable Read+Seek input. Check the declared bound first. Preserve original owned
random staging, file fsync, complete byte comparison, preselection descriptor
duplication, no-replace rename, parent fsync and uncertain selected outcomes.
Use two bounded buffers rather than changing byte equality to a weaker checksum.
Require exact source EOF in each pass and stable stage metadata across readback.
Keep existing byte publishers untouched.

Use the new publisher with ArchiveReader over the native backup's immutable
capture. Preserve complete current source inventory and destination admission,
actual selected descriptor, complete final archive verification and final retained
source-marker check. No new stored bytes/version, transaction acknowledgement,
network upload, account-root schema or user authority is selected.

## Consequences and acceptance

Caller-supplied immutable source and native filesystem trust remain explicit.
Read/seek interruption is not a time bound. Capture and standalone restore retain
owned images; this is not a whole-process quota or coordinated platform backup.

Require exact empty/chunked/Interrupted reads, read/seek errors in both passes,
short/trailing/changed input, full byte mismatch and late stage edits. Preserve
foreign substituted stages and selected entries. Inject before/after file and
parent fsync failures. Kill actual processes during copying, after file fsync,
after selection and after received empty/full success. Compare actual maximum
CLI backup and independent restore inventories. Any memory observation must retain
raw fresh-process samples, unchanged sources and exact identical output bytes,
including a small shape. Run supported toolchains and record source hashes.
See [contract](../reader-file-publication.md).

Executed199 relevant checks per stable/minimum on580 frozen hashes. Eleven new
cases include64 generated sources, five new process kills and exact maximum CLI
backup/independent restore. Reproduced subprocess/lock-test interference is fixed
by the existing test serialization contract;20 full native library repetitions
and both final matrices pass. Failed attempts remain excluded and documented.
Fresh actual CLI large-payload median RSS falls138304 to73396 KiB; the small shape
has no improvement. Raw samples/method and source boundaries are in the contract.
No parser sanitizer, whole-workspace or production result is added by these checks.
