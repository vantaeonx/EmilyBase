# ADR0073: private restore validation and reset before publication

Status: accepted for separate private account archives; platform restore pending.

## Problem

Generic EMILYBAK restoration faithfully retains session scope/time and verifiers.
A formerly current token can authenticate under restored metadata until explicit
reset. Publishing that directory first and resetting afterward exposes an unsafe
ordering for a future service. An engine-valid archive may also contain invalid
private account schemas or the wrong project binding.

## Shared owned preparation protocol

Extend the existing descriptor-owned backup publisher with restore_prepared.
Read and validate a bounded regular archive, stage its WAL privately with mode0600
inside a mode0700 directory and sync it. Verify its exact acknowledged prefix and
replay under the archive database identity. Run one trusted synchronous application
callback against that descriptor-anchored private directory. It must close all
database owners and must not expose the path as a published application store.
No network/user-selected callback is allowed by this API.

After preparation, verify destination/staging identities again, reopen the same
owned directory under the original database ID and validate every committed page.
Build the checkpoint from prepared state, sync the staged directory and publish
without replacement through the pinned parent. Sync and recheck the parent before
success. The resulting report describes the installed prepared journal, which may
have more transactions than the source archive. The archive remains immutable.
Ordinary restore uses an infallible no-op callback through this shared protocol
and retains its previous exact data/report semantics.

PreparedRestoreError distinguishes application refusal from underlying restore
failure. Refusal or pre-publication failure leaves no final directory and cleans
only the still-owned staging entry. Foreign substitutions are never removed.
Once rename has occurred, sync/selection failure preserves the installed state
and reports PublicationUnknown; no rollback or automatic replacement is attempted.
Private remnants from a killed process may remain inspectable; no automatic
orphan sweeping or claim of resumable staging is introduced.

## Private account restoration

restore_private_accounts validates trusted project/time parameters before any
staging, opens the private store with full version-selected semantic validation
and enforces its expected project ID. Version3 resets incarnation and clock in
one original WAL transaction. Versions1/2 explicitly activate version3 with a
fresh incarnation. Accounts/password verifiers/epochs/disable state are retained;
old family rows remain bounded history, without old authentication authority.
The reset occurs before checkpoint/sync/final publication. Trusted time can be
lower than the old watermark only because scope changes with it.

The restored archive must have WAL headroom for the mandatory reset/migration;
commit failure refuses publication. No silent compaction/downgrade is attempted.
No plaintext token is reconstructed or returned. An independently reopened final
store immediately rejects prior access/refresh while retaining real password login.
A verified backup of that installed store preserves the reset and rejection.

## Verification and open gates

Tests cover private versions1/2/3 on both WAL versions, immutable source/WAL,
old token denial, current password/account state, restart, verified recapture,
wrong-project and engine-valid/private-invalid archives, invalid trusted input,
existing destinations and symlink input. Independent64-case row/commit/rollback
and32-case account epoch/disable models check prepared and restored state.

Four forced process kills cover reset-before-publication and acknowledged
publication on both WAL versions. Before publication no final store appears;
after acknowledgement the complete reset is present. Two synchronized private
restorers reach the prepared barrier and exactly one complete new scope is
published. Waits are bounded, source state remains exact and neither preparation
exports a credential. This is not physical power-loss testing.

Prepared publication fault cases cover before/after WAL, directory and parent
sync on both WAL versions:12 combinations. Parent/staging substitutions, changed
prepared database identity, corrupted WAL, held callback owner, application
refusal after a commit and post-rename selection changes fail at the proper
boundary. Existing ordinary publisher recovery/ownership tests still execute.
Actual toolchain/strict-check results are in testing.md and the source-bound artifact.

This API restores one separately captured private store. Current registry archives
still do not capture it or coordinate account/data consistency. Combined capture,
platform publication/upgrade, HTTP account workers/rate policies, roles, row
policies, memory quotas and broader security/load/fault gates remain open.
No file/page/WAL/EMILYBAK version changes and no platform milestone closes here.
