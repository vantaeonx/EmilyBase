# Selection-aware native file mutation checks

[ADR0175](adr/0175-selected-native-file-mutation-checks.md) adds synchronous
`NativeFileRoot::rename_with_checks` and `remove_with_checks`. These are trusted
native library operations, not public user endpoints or an installed policy
implementation. The existing AccountRoot user gateway supports reads only.

## Callback and ordering

The higher-ranked callback accepts `NativeMutationCheck<'_>` and returns the
native typed Result. `Selected(&FileInfo)` borrows the actual validated logical
reference inside the mutation. `Phase(NativeWritePhase)` carries the existing
write boundary. Neither event exports a database or directory handle.

For a valid existing reference with a changed name or a removal:

1. Validate bounded name where applicable and original native state.
2. Call Phase(BeforeOperation).
3. Validate the actual metadata/object graph and select the requested reference.
4. Check original selection, call Selected and check original selection again.
5. Compare exact current revision, retain/verify the original object reader and
   stage the metadata transaction.
6. Call Phase(BeforeCommit) under original source checks.
7. Commit the original metadata WAL, accept its exact original inode/readback and
   call Phase(Committed).
8. Verify the resulting graph and retained reader, call Phase(Verified), then
   verify final original native state before returning the native receipt.

Selected runs exactly once for an existing reference, including a stale revision
or an equal-name no-op. A no-op has only BeforeOperation, Selected and Verified;
it retains exact WAL bytes. Missing IDs have only BeforeOperation and return
Missing. Invalid names fail before callbacks. No metadata edit emits BeforeBlob
or publishes/deletes any physical object.

Selection authorization precedes CAS so a refused caller cannot obtain a stale
revision result from a reference it was never permitted to modify. The native
library still exposes Missing/Conflict and sensitive native receipts. A future
user gateway must authenticate before invocation, apply current per-action
policy to Selected's owner, hide missing/denied distinctions as required and
return only its chosen public write receipt. A callback that returns Ok is
trusted coordination, not proof of a real user. Copying metadata grants no later
permission; the original reference borrow cannot escape the callback.

## Failures and data retention

A healthy BeforeOperation or Selected refusal does not stage/commit metadata or
poison the root. A revision conflict has the same no-write behavior after a
successful Selected check. Native staged-write refusal conservatively requires
reopen; committed failures are OutcomeUnknown. A refused verified no-op preserves
the healthy exact source. A panic at any trusted callback poisons the native
owner before propagating, even if no write occurred.

Original source checks run after a callback even if it returns an error. Replacing
a root, component directory, WAL or scope file cannot be hidden behind a policy
denial. Source failure poisons the native owner. Equal bytes do not make a new
inode the original selection, and an in-place rewrite is not accepted as the
original WAL. Foreign bytes are preserved for inspection; the operation does not
repair, clean up, retry or adopt them. These checks are finite observations,
not a namespace lease against a malicious host.

Rename preserves logical ID, object, owner, payload report and quota. Removal
changes logical visibility only; all physical orphans stay charged. A removed
logical ID can later be republished with another object/owner by a trusted native
caller. The next Selected event sees that actual new reference, never an earlier
copied owner. Existing phase-only guarded and unguarded APIs remain available.

## Executed verification

See the source-bound [verification record](measurements/2026-10-27-selected-file-mutations/verification.json)
for actual toolchain results. Tests exercise both WAL versions, original owner
descriptors, denial before CAS/no-op, reused logical IDs, callback errors/unwind,
namespace and in-place WAL substitutions, independent verified restore, generated
histories and real process termination at each callback and caller-return boundary.

No parser, stored format, migration, dependency or durability acknowledgement is
changed. User uploads/edits, HTTP/SDK file endpoints, complete container v2 drills
and all unmet release/security criteria remain open. Fixtures are synthetic.
