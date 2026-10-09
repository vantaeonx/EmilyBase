# ADR0110: explicit closed public admission catalog

Status: accepted for explicit native operator metadata after these checks.

## Context

The existing owned row gateway and HTTP adapter require both a project service
key and a current user credential. Mobile/browser clients must never receive a
service key. User-only admission needs a separate explicit operator decision
before any keyless network entry point is implemented. Restoring a formerly open
project must not accidentally expose its copied data under a new deployment.

## Decision

Add private account schema5 through an explicit v4-to-v5 migration in the original
synchronous engine. Preserve the exact seven v4 tables and add exactly one bounded
singleton auth_public_admission table. Its initial enabled flag is false. Migration
creates the schema, singleton and scope version in one original WAL transaction;
users, policies, session incarnation and clock remain unchanged. Versions1..4 remain
readable without implicit upgrades. Older readers refuse5. Existing explicit
session/clock/policy enable operations never downgrade5.

The singleton has id INTEGER1, record version INTEGER1, enabled BOOLEAN, and two
canonical unsigned decimal TEXT fields: revision and previous. Revision is the
actual private commit LSN, positive and no greater than the inspected committed
prefix. Previous is zero initially or strictly less than revision. An initially
enabled row with zero predecessor refuses. Text preserves identifiers beyond i64;
the original WAL reserves u64::MAX, so a new write at an exhausted LSN refuses.
Complete open/archive/root validation checks the exact eight schemas, singleton,
canonical values, revision bounds and all previous private referential constraints.
Original database/page/WAL/backup/root-bundle bytes and versions remain unchanged.

Expose metadata and current-owner CAS through AccountStore and retained AccountRoot.
Root operations require the current project service key and existing selected
filesystem identities. Migration refuses before v4. A flag change requires the
current revision. An identical retry accepts only the current revision or recorded
predecessor and returns the same receipt without writing. A stale command cannot
reopen admission after an intervening open/close cycle. Unrelated clock/session/user
commits do not replace the flag receipt. Migration retry preserves an already open
flag; it does not silently close a running project.

A receipt is cloneable operator metadata, never an authorization capability. No
user-only request gateway, public HTTP route or CLI command is added in this block.
Existing trusted service-key routes retain their explicit authority. A subsequent
user gateway must read the current flag under the actual private owner, verify the
current user session and enforce the installed current table policy while retaining
both original owners through the complete data operation. Missing catalog, closed
flag or missing policy must deny. Public signup, roles and arbitrary user SQL remain
separate decisions.

## Restore and uncertainty

When reset_session_clock replaces an incarnation/time in a v5 store, it closes an
open flag in that same original transaction. The closure receives that actual
commit LSN and its previous flag revision. If already closed, preserve its receipt
while committing the independent session reset. Verified private/common-root restore
runs this operation in its owned staging directory before publication. Thus a copy
preserves accounts, policy groups, service-key digests and data, but closes admission
and revokes old user sessions. Its source remains untouched and can remain open.
Generic engine restore deliberately preserves original metadata and requires an
explicit reset before accepting traffic. No cross-database atomic restore is added.

Original WAL acknowledgment, poisoning, limits and publication uncertainty apply.
Do not automatically retry a failed write or equate a lost result with rollback.
The exact metadata/CAS can inspect a reopened committed state. The catalog is not
an audit history, an authenticated receipt, a network admission implementation or
a proof of production readiness.

## Verification plan

Both Rust stable and1.89: complete private-auth regression, common-bundle/root
regression, private HTTP regression and offline user/policy/key CLI suites. New
checks cover migration/session/policy preservation, legacy refusal, exact retries,
ABA, enabled-source restore and operator reset, fifteen valid-engine semantic defects,
large unsigned revisions and exhaustion, an independent generated state machine and
sixteen staged/caller-received process kills across both WALs. A native two-project
root test covers current-key/sibling isolation and nonempty verified clone. Extend
private-account archive sanitizer fuzzing with structured v5 admission defects.
Strict workspace/fuzz formatting/linting and minimum compilation remain required.
HTTP/CLI policy-enable retries must report the actual private version4 or5. New
regressions first observe both adapters returning4 from an unchanged v5 store;
read current guarded schema metadata instead and widen the documented response
enum. This response change exposes no public-admission administration route.

The new root test first reproduces a missing v5 compatibility admission in the
original root inspector; extend its accepted private range to3..=5 while preserving
all complete private/clock/database-identity checks. The same test must pass before
publication. Invalid high-LSN test fixtures cannot claim that u64::MAX is a valid
WAL baseline: the original format intentionally reserves it.


## Final verification

On Rust1.99 and1.89,305 checks pass per toolchain: complete auth147, original
common-bundle/root94, private HTTP40 and actual user/policy/key CLI24. Eleven
regular cases are new. The final private-archive ASan campaign executes40595
runs in46 seconds without findings. Workspace/fuzz formatting and strict Clippy,
minimum workspace build and all-target fuzz compilation pass on490 frozen
source/dependency/OpenAPI hashes. The block adds987 Rust lines; total source103595
(Rust99308, SDK2502, Python1785). See
[source-bound evidence](../measurements/2026-10-09-public-admission-catalog/verification.json).

A superseded stable run had two self-spawn ENOENT failures during executable
replacement by an overlapping rebuild. An isolated unlink probe reproduces that
Linux behavior. Final suites run on unchanged frozen sources without replacing
their test executables; no product-engine fix is inferred from this interference.
Public admission enforcement/network, roles, resources/load/upgrade and independent
security/production acceptance gates remain open.
