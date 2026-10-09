# ADR0122: isolate the token allocation observation

Status: accepted for experimental synthetic-data use.

## Context

Hosted run37967197534 failed the session-token zero-allocation observation with
144 bytes in two blocks. Its sample used the process-wide allocator inside
libtest. A sequential test runner does not exclude unrelated harness threads.
An unchanged local run passed; controlled background work during the original
sample reproduced a false token attribution:328 bytes/six blocks,144 live bytes.
The exact origin of the hosted two blocks was not traced; the controlled result
demonstrates that the old observation cannot distinguish these sources.

## Decision

Apply the existing standalone schema-diagnostic pattern to token measurements.
An opt-in native binary owns the process allocator, prepares synthetic fixtures
before measurement, exercises the same1000 match/parse/decode iterations, then
separately observes issuance and release. No test-harness thread shares its
allocator. Emit bounded count-only JSON after both profiler lifetimes.

Keep strict zero matching/decode allocation and exactly one102-byte issuance
owner with zero live bytes after drop. Add an explicit144-byte negative control
inside the measured operation; it must still fail. Integration tests check both
outputs/statuses and retain unrelated allocations in the parent process. Invalid
diagnostic arguments refuse without sample output. The original auth code,
credential bytes, token format and database durability are unchanged.

## Verification and limits

Final evidence will record controlled failure before isolation, both Rust versions,
full opt-in release diagnostic suites, repeated fresh native observations and
strict lint/default-feature/fuzz compatibility. The fixture is synthetic and
count-only; no plaintext credentials are printed or persisted. This is an
operation allocation observation, not RSS, throughput, service memory admission,
cryptographic timing certification or a completed security audit.


Executed on556 frozen hashes: full opt-in release diagnostics56 and default
report checks18 on each stable1.99/minimum1.89.0; strict workspace/opt-in/fuzz
lint, both formatting checks and minimum all-target diagnostic/fuzz compatibility
pass. Each toolchain runs100 fresh normal samples and one explicit negative:
match/decode zero, issuance102 bytes/one block, release zero; negative144 bytes/
one block exits1. Parent background-allocation regression passes with zero
counter tolerance. See
[verification](../measurements/2026-10-09-token-allocation-isolation/verification.json).
