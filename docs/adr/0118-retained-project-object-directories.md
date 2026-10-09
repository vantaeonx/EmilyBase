# ADR0118: retained native project object directories

Status: accepted for experimental synthetic-data use.

## Context

The scoped object envelope validates bytes, but a caller-provided filesystem path
does not establish a project namespace. Re-resolving a directory path for every
operation can redirect writes when that pathname moves. Network authorization
must remain separate from native filesystem authority.

## Decision

Open an operator-provided existing 0700 directory with final no-follow admission
and an exclusive cooperating-owner lock. Publish a fixed exact empty EMILYOBJ
scope marker through the original storage stage. Retain the original marker and
directory descriptors. Require current marker inode, expected scope and private
admission before/after operations. Reject missing or malformed state without
creation/repair. Preserve pre-existing unmanaged entries without treating them as
validated inventory.

Expose only typed object IDs. Derive single-component immutable filenames inside
the owned directory and extend the original byte publisher with a directory-handle
entry point, sharing all staging/readback/no-replace/sync code. Reads return owned
fully verified bytes and redact payload in Debug. Directory pathname moves do not
redirect existing owners. No AccountRoot, HTTP or user authority is implied.

Use an explicit unlock guard covering successful and failed construction paths.
A parallel generated-history preflight observed Busy after dropping an owner;
the isolated model passed. A deterministic regression duplicating the open-file
description reproduced the extended lock lifetime before the fix. This models
fork inheritance without unsafe fork/pre-exec code; the actual transient fork
window was not directly observed. Releasing ownership explicitly fixes that
reproduced descriptor case and the final concurrent test suite must pass.

Add native CLI init/put/inspect. Buffer redirected bounded binary input before
locking, validate IDs before input, and print metadata only. Failed stdout after
publication requires inspection; it does not imply rollback. Existing names never
overwrite. A temporary-borrow compilation error in the first property fixture was
fixed before runtime checks; it is separate from the reproduced lock defect.

## Verification and limitations

Final evidence will record both toolchains, native models/current substitutions,
real CLI/binary bytes/output failures, shared storage faults and received-result
kills. Existing envelope/page-file tests rerun. Format and strict lint plus all-fuzz
compilation cover integration. The pure envelope parser is unchanged; no new
ASAN run is claimed for directory operations.

The directory remains experimental native filesystem authority. Cooperating
exclusive locking is not authorization against administrators. Scope checks are
snapshots; marker hashes do not authenticate hostile writers. Root/project roster,
object policies, HTTP, quotas, inventory/delete/cleanup, signed URLs, verified
backup integration and production acceptance remain open.


Executed64 Rust checks each on stable1.99/minimum1.89.0:24 object,33 storage
and7 actual CLI cases. Three helper workers are explicitly invoked by parent
processes. Seventeen new regular cases include24 configured immutable histories,
three new received-result kills (scope/empty/full), two real stdout failures and
four new shared sync injections per toolchain. The old two object and three raw
creation kills rerun. The deterministic lock test fails before and passes after
the guard fix; failed-construction ownership is covered too. Workspace/fuzz format,
strict lint, stable CLI/server build, minimum workspace build and all-fuzz compile
pass on544 frozen hashes. Pure format parser unchanged; no new ASAN run claimed.
See [verification](../measurements/2026-10-09-object-directories/verification.json).
