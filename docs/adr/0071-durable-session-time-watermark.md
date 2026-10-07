# ADR0071: durable session time watermark and atomic reset

Status: accepted for local metadata protocol; session admission and HTTP pending.

## Problem and versioned change

A future expiry check must not accept a credential again after the clock moves
backward, including after restart. Metadata parsing alone cannot enforce this.
Persist the highest trusted observed integer-second timestamp before session
checks and refuse lower timestamps. Client input must never select this clock.

Private schema version3 adds auth_session_clock with non-null integer primary id,
integer version and integer observed. Exactly one row has id1/version1 and
observed in0..=i64::MAX. Version3 requires exactly five schemas, preserving all
version2 account/session columns and EBSK bytes. Creation still uses version1;
opening supports private versions1/2/3 and rejects unknown/partial inventories.
No automatic migration occurs. Older readers reject version3; no downgrade
rewrite is offered. Page/WAL/EMILYBAK versions stay unchanged.

Explicit enable_session_clock(now) atomically creates missing v1 session schemas
or retains v2 family history, inserts the clock and selects private version3.
Activation always chooses a fresh incarnation, invalidating historical v2 scopes
before a future clock-aware session service can use this storage. Cached scope
and watermark publish only after mandatory WAL commit/sync. Repeating activation
at the same time is a no-op; a later supplied time advances the watermark, while
a lower/out-of-range value is refused. The old enable_session_storage call
preserves already activated metadata rather than downgrading it.

## Observe and reset

advance_session_clock(now) requires explicit clock activation and a trusted
server/operator timestamp in0..=i64::MAX. Equal timestamps preserve exact history;
a higher one commits the new clock before returning. A lower one fails without
staging and remains refused after compaction, restart and verified restore.
Every future credential attempt, including denied/expired attempts, must observe
this watermark before credential/expiry checks. Without that integration the
metadata primitive alone does not prevent a caller bypassing clock observation.
The eventual network API must obtain time internally and offer no client override.

A large unexpected forward clock jump fails closed on subsequent earlier time.
The trusted operator can reset_session_clock(now), which replaces incarnation and
watermark in one WAL transaction. This permits a deliberate time correction or
restored-state reset while old credentials lose the independently selected
current scope. Never lower the watermark without that simultaneous invalidation.
A new incarnation excludes the current one and all retained family incarnations,
with at most four OS-random attempts; exhaustion/entropy failure leaves history
unchanged. Retained sets are bounded4096; no numeric total-heap quota is claimed.

Opening version3 validates the clock completely. Current-incarnation family rows
cannot claim issue times above the committed watermark. Old-incarnation records
may remain historical data, including times above a reset watermark, because
future admission must refuse their scope. Epoch/identity/deadline validation
retains its independent prior rules. Pure inspect_session_clock_record returns
untrusted metadata only, without storage access or granting permissions.

The protocol writes at most once for each increased integer-second timestamp
within the intended service scope; equal observations do not grow the journal.
It adds synchronous commit cost when time advances and still needs bounded
worker admission, coordinated checkpoint/compaction and workload measurements.
A trusted future clock is not a universal availability/latency guarantee.

## Restore and open gates

Generic backup faithfully restores the old watermark and incarnation. It does
not run reset automatically. The explicit reset proves old token verifiers fail
under the newly selected scope, but coordinated account/data restore must invoke
this step durably before traffic and handle uncertain/failing publication.
Existing registry archives still do not capture separate account stores.

No runtime sign-in/refresh/logout/pruning or session principal is implemented
here. Those operations must bind current account ID/epoch/disabled state, observe
time, verify secrets, enforce deadlines and commit rotation before acknowledgment.
Reopening metadata is not authentication. HTTP/rate/cookie controls, numeric
memory admission, broad fault campaigns and production gates remain open.

## Verification

Tests cover explicit activation from both prior private schemas on both WAL
versions, old snapshots, equal-time/no-op history, forward commits, backward and
out-of-range refusal, compaction, restart and verified five-schema backup/restore.
Atomic reset changes scope/time together and preserves accounts; restored old
verifiers fail under the new expected scope. Deterministic entropy candidates
exercise current/historical collisions, four-attempt exhaustion and refusal
without weakening the production OS generator.

Ten invalid but engine-checksummed clock/schema fixtures fail opening. Current
and historical family issue-time constraints differ explicitly. An independent
64-case sequence model covers advance/refusal/reset/history and independently
restored state; a separate biased shape/bounds property exercises valid clocks.
Twelve forced kills cover staged/acknowledged activation, advance and reset across
both WAL versions. Recovery and independently restored recovered backups retain
exact scope/time; staged cases keep exact prior journal bytes. This is not a new
physical power-loss/mid-fsync campaign. Parser-only fuzzing, native requested-heap
observations and final actual check results are recorded in testing.md.
