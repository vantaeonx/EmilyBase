# ADR0175: authorize the selected native file before revision matching

Status: accepted, 2026-10-27. Scope: synchronous native file library only.

## Problem

The current user read gateway consumes a retained reader's actual owner. A future
rename/removal gateway needs the same property at the actual mutation. Calling
`file_info` in an earlier operation and then authorizing a copied owner is not a
safe API contract. The same logical ID may have been removed and republished.
The old phase-only coordination callback never receives the selected reference.

## Decision

Add `NativeFileRoot::rename_with_checks` and `remove_with_checks`. Their trusted
callback receives either a borrowed `Selected(&FileInfo)` event from the actual
validated mutation or an existing `NativeWritePhase`. Selection occurs before
revision comparison, retention of the edit's object reader, no-op acceptance or
transaction staging. Graph validation can inspect payloads before this event;
outer authentication must precede the entire native operation.
The callback cannot access the metadata database or write a different
reference through this borrow.

Original native selection is checked before and after every callback. A callback
error cannot hide a changed source. A healthy selection denial preserves exact
WAL bytes and native usability. Post-commit error remains OutcomeUnknown; unwind
closes the native owner and propagates. Removal preserves physical objects and
their quota charge. Existing phase-only and unguarded operations retain their
contracts; they do not gain user authority.

This is a trusted native coordination primitive, not a session or policy proof.
The caller must establish current user/admission/policy context and map missing
and denied results before exposing them. Independent action rules and minimal
user receipts belong in the later AccountRoot gateway. No user write endpoint,
schema migration, dependency, format version or production gate is added here.

## Validation

Exercise both existing WAL formats, selected-owner denial before stale CAS and
equal-name no-op, ID reuse with a new actual owner, source changes in callbacks,
trusted errors/unwind, actual killed writers, descriptor lifetime, independent
reopen/restore and a generated mutation history. Preserve the existing native
publication, metadata, quota and server adapter tests.
