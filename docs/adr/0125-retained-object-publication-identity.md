# ADR0125: retain selected object identity through the result boundary

Status: accepted for experimental synthetic-data use.

## Context

Archive publication already retains its selected file. Original standalone object,
project marker and native put paths instead reopened a final name after storage
publication. A valid identical replacement could pass final byte/report checks and
receive success attributed to a file the operation had not selected/synced. Bounded
put adds a later receipt boundary and must not discard the inode after inner put.
Four deterministic substitution tests reproduce these distinct boundaries.

## Decision

Add a path-based retained-file storage API over the existing original publisher.
Preserve unit-returning APIs and clone before selection. Read standalone objects,
markers and native objects through the actual selected descriptor and compare
visible identity as well as full contents. Carry that descriptor through bounded
put's final inventory and exact payload/report checks. Map observed postselection
changes to the existing uncertain outcome; never retry, overwrite or delete.

## Consequences

The original encoded bytes, fsync ordering and preselection behavior remain stable.
A descriptor lives until the enclosing result is complete. Bounded writes perform
an additional complete bounded object read after the global inventory. This is not
a whole-process memory reservation or a persistent service quota.

Native operators/ancestors remain trusted; final checks provide no namespace lease.
Current root bundles still exclude objects, and user file HTTP/policies/signed URLs
remain separate gates. Parser sanitizer and physical power-loss claims do not
follow from this change. See [contract](../object-publication-identity.md).


Executed156 relevant checks per stable1.99/minimum1.89.0 on568 frozen hashes.
Four regressions fail before their respective corrections and pass afterward;
eight new regular cases include original sync-fault outcomes. Existing20 kills
and generated models rerun. Strict formatting/lint/build/minimum fuzz compilation
pass; no new ASAN campaign or full-workspace result is claimed by this evidence.
See [verification](../measurements/2026-10-10-object-identity/verification.json).
