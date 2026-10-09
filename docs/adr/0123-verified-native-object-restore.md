# ADR0123: verified native object directory restore

Status: accepted for experimental synthetic-data use.

## Context

A private checked archive is useful only if a complete independent directory can
be reconstructed and verified. Existing root/server directory publishers are
private to higher layers. Depending upward on them would couple storage to the
platform and risk a dependency cycle. Recursive cleanup of a substituted or
partly populated directory also needs authority we do not possess.

## Decision

Add an original storage-level owned0700 directory stage. Retain parent/stage,
refuse final-parent symlinks, check exact identities/private mode, fsync the
caller-verified populated directory, select with the original no-replace rename,
fsync the retained parent and return selected directory/parent identities.
Clone descriptors before selection. Drop removes only unchanged empty stages;
nonempty or substituted content is preserved for explicit inspection.

Verify complete object input before staging. Reconstruct the original project
marker and every scoped canonical object through retained descriptors and the
original private file publisher. Verify complete inventory before and after
selection; fully reread/recheck native archive input before selection. Refuse
rescoping/overwrite/merge. Distinguish incomplete unselected stage from selected
unknown results. Add an offline metadata-only restore CLI.

Original database/object/archive bytes and version rules remain unchanged. This
standalone restore does not change AccountRoot archive membership or enable HTTP.

## Verification and limits

Final evidence will cover both Rust toolchains, maximum complete native restore,
independent copies/generated histories, malformed input before any staging,
source/stage/parent/final-name mutations, original sync faults, concurrent restore,
real process kills and actual CLI stdout failure. A preflight property assertion
needed a local binding for a borrowed temporary; this was a test compilation
fix, not a runtime product failure.

Native administrators/ancestors remain trusted; observations are not leases or
hostile-filesystem transactions. Nonempty abandoned stages require manual
inspection; no automatic janitor is supplied. Multiple image buffers can coexist.
Process kills are not power-loss tests. Root integration, file policies/HTTP,
quotas, encryption/signed URLs and production gates remain open.


Executed132 checks each on stable1.99/minimum1.89.0:70 object,42 storage and20
actual CLI. Twenty-one new regular cases include24 generated restore histories,
four competing complete restores, maximum128-object/64 MiB native file restore
and full reencoding, all-prefix/single-byte refusals before staging, source/stage/
parent/final mutations, and actual CLI stdout failure. Four new process kills
distinguish unselected private stages, selected/unreturned and received empty/
nonempty results; twelve original kills rerun. Four new directory/parent sync
faults preserve selected-unknown versus unselected outcomes.

Workspace/fuzz format/strict lint, stable CLI/server build, minimum workspace
build and all-fuzz compile pass on562 frozen hashes. No new ASAN execution is
claimed: pure decoders/formats are unchanged, and previous parser-only ASAN does
not fuzz this filesystem layer. See
[verification](../measurements/2026-10-09-object-restore/verification.json).
