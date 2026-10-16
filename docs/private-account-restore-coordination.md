# Private account restore under the original database owner

`with_restored_private_account_bytes_at(bytes, project, parent, leaf, pool, now, work)`
is a synchronous native Linux operator API. It validates an immutable private
backup, moves the actual restored Database into a private AccountStore adapter,
commits a fresh session scope and trusted time, and keeps the original writer/WAL
through child selection and report-only nested work. It does not restore a complete
platform root or grant current user authority.

## Preparation

Validate the project, trusted timestamp and complete private snapshot before creating
any stage. `now` must fit the existing signed timestamp domain and comes from the
trusted operator, never a client parameter. Require an actual0700 native parent and
one1..255-byte leaf through the existing core publisher. Existing targets are never
replaced. Moving the original parent does not adopt a replacement at its former path.

AccountStore's private constructor validates the supplied database directly. It never
closes/reopens that owner by a filename. The public store does not implement
PreparedDatabaseOwner: only the private adapter implements the mutable getter, so
callers cannot escape the account schema/session invariants through this API.

A borrowed core PreparedSourceGuard checks the original creator reader's complete
bytes, physical length/timestamps, single-link0600 WAL identity, selected private
stage and parent immediately before session preparation. This is exposed through
`with_guarded_prepared_owner_bytes_at`; the original trusted factory API remains
available with its previous contract. The guard cannot acknowledge newly committed
history and exports no file/database descriptor.

This extra source check matters: ordinary WAL writes may discard an abandoned tail.
An isolated failing test showed that a tail appended during readonly account
construction could otherwise disappear during the reset and yield success. The
adapter now refuses that observed change before writing. Ordinary recovery and WAL
acknowledgement semantics are unchanged. Finite checks under trusted OS ownership
are not namespace leases or a sandbox for malicious callbacks.

Private versions1/2 activate version3, preserving users, password/dummy verifiers and
historical family rows. Versions3/4/5 keep their version. One durable preparation
transaction sets a fresh random session incarnation and the supplied time; version5
also closes public admission in that same transaction. Row policy receipts, disabled
state, account epochs and existing family records remain. Old credentials cannot
gain authority in the copy; original source credentials/data remain unchanged.

A trusted restore may intentionally choose a lower time than the source watermark
because its session scope is fresh. This differs from ordinary nondecreasing runtime
clock advancement. The prepared private report must match the original database ID,
account/family counts, expected version and exact new time, and must agree with the
core's recovered prepared report before work starts.

## Coordination and failure

The user callback receives only validated PrivateArchiveReport metadata. It may
prepare other components and select an enclosing root while this same original
private database remains locked. Actual common-root roster/selection and aggregate
resource admission belong to the caller. This function never exposes AccountStore,
raw database mutation, tokens or password verifiers to callback work.

After work, the core repeats original directory/WAL identity, bytes, length, privacy,
timestamps, acknowledged history and selected-name proofs, including WAL/parent
confirmation syncs. Values wait for these checks. A callback Result is a nested value;
propagate both layers. A healthy callback refusal leaves a complete fresh copy.

Observed postselection failures are typed RetainedRestoreUnknown, either at the auth
adapter or wrapped core backup layer. Discard unconfirmed values and preserve selected,
foreign and nonempty private stages for operator inspection. Do not overwrite, retry
blindly or promise rollback of an already acknowledged preparation. Earlier failures
withhold selection/work; an interrupted stage may contain either original history or
the acknowledged reset. Compaction/rebinding during preparation remains unsupported.

## Verification and remaining work

[ADR0160](adr/0160-private-account-owner-restore-coordination.md) records the choice.
[Source-bound evidence](measurements/2026-10-16-private-owner-restore/verification.json)
records actual final processes and their exclusions. Ten new auth cases cover all
five private versions with WAL1/2, exact records/verifiers/policies, disabled users,
legacy historical families, original credential validity, copied credential refusal,
new sign-in/reopen, two nested private owners and independent copied scopes.

Sixty-four physical mutations span four account boundaries and eight WAL/root/
parent changes across WAL1/2. Before-reset tails are refused without changing the
original source. Bounds, invalid private images, wrong scope/time, occupied targets,
parent moves and callback refusal are checked. Sixteen generated models vary version,
WAL and empty/nonempty disabled-user sets. A separate core case proves a source guard
cannot acknowledge a later preparation commit.

Ten actual signal9 kills cover owner-open, acknowledged reset, child-selected,
work-complete and returned boundaries for legacy v1/WAL1 and v5/WAL2. Copies reopen
with exactly the applicable original/prepared transaction, original credentials remain
valid, copied old credentials fail after reset, and fresh retries/sign-in work. Original
locks and at least two original WAL descriptors survive through pre-return phases and
are released after return. Reused ignored workers are parent-launched fixtures.

The full joined account/file root restore, v2 live startup/adoption, aggregate process
admission and production acceptance remain open. Formats/dependencies and byte parsers
are unchanged. No new sanitizer, SDK, container or deployment claim is implied.
