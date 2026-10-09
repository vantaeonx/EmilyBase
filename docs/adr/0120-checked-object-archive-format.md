# ADR0120: complete checked object archive bytes

Status: accepted for experimental synthetic-data use.

## Context

Native capture now supplies bounded immutable object bytes, but an archive must
frame the complete selected set, reject missing/duplicated/cross-project contents
and preserve the same canonical inventory identity. Encoding bytes alone cannot
close backup publication or restore acceptance.

## Decision

Define experimental EMILYOBK v1 with an exact128-byte scoped header, framed ordered
EMILYOBJ images, full body SHA-256/header CRC32 and the original inventory digest.
Bound count128/payload64 MiB/complete bytes67,124,352. Verify every frame and nested
project/object/length/checksum, strict ID order, exact count/total and canonical
inventory digest before exposing immutable borrowed payloads. Unknown versions
fail closed. Share unchanged digest framing with the native inventory.

Encode only private-constructor captures or completely verified archive views.
Use fallible bounded output allocation. Preserve canonical exact bytes on
re-encoding. Reuse private readonly file admission with an explicit archive-size
bound, before/after metadata rechecks and visible inode validation. Add only a
metadata inspector CLI; do not label this a durable archive/restore implementation.

## Verification and limits

Final evidence will record both toolchains, independent bytes, all prefixes/byte
corruption, checksum-resealed structural failures, foreign nested scope, strict
set order, maximum combined count/bytes, native file inspection, generated models,
actual CLI and ASAN. The fuzz target also recomputes untrusted outer checksums to
reach deep structural validation; that is not authentication or a bypass.

Archive checksums do not authenticate or encrypt data. Borrowed views are not user
capabilities. Native paths/admins are trusted, inspection is not a retained lease,
and logical byte bounds do not reserve global heap. Existing root backup formats
remain unchanged. Durable publication, restore/copy isolation, root integration,
streaming/compression/encryption, HTTP/file policies and production gates stay open.


Executed92 checks each on stable1.99/minimum1.89.0:46 object,33 storage and13
actual CLI cases. Eleven new regular cases include independent complete bytes,
24 generated archive histories/24 arbitrary draws, full128-object/64 MiB exact
67,124,352-byte archive including native file inspection, resealed structural
failures and actual readonly stdout failure. Three harness workers are explicitly
invoked by parents; all eight existing kills rerun, no new durable backup claim.
Workspace/fuzz format/strict lint, stable CLI/server build, minimum workspace build
and all-fuzz compile pass on551 frozen hashes. ASAN executes2,083,712 inputs in
46s, RSS404MiB under512,max262144/prefix16,776 seeds,
raw and outer-resealed branches, no findings. Fuzz is smaller than the separately
tested maximum archive and proves neither publication nor restore.
See [verification](../measurements/2026-10-09-object-archive/verification.json).
