# Native file metadata compare-and-swap

FileStore supports `rename(id, expected_revision, name)` and
`remove(id, expected_revision)` under its original metadata/object owners. Both
require the exact current reference revision. Missing references return Missing;
stale revisions return Conflict before any commit. Invalid display names also
refuse before staging. These are trusted native operator methods, not user policy.

Rename changes only bounded display text and the actual commit revision. Immutable
object identity, bytes/hash, owner metadata and physical quota remain. A matching
current name is a no-op only after revision matching and native verification;
it preserves the exact WAL. A stale command cannot become a successful no-op
merely because its requested text matches current text.

Remove commits logical reference deletion. FileRemoval returns the previous opaque
FileInfo plus the actual deletion commit revision. The object is not deleted and
physical quota is not freed. List/info/reader stop exposing the reference; usage
counts the surviving valid object as an orphan. There is no tombstone or automatic
retry receipt: another remove of a missing ID is Missing. A logical ID may later
be republished with a fresh object and newer revision; an old revision cannot
remove or rename that generation.

Operations validate current metadata/inventory and retain the actual object reader
through staging, commit and final checks. After staging, source verification
precedes the commit attempt. A detected change there discards the transaction and
returns a direct error; FileStore requires reopen, but no commit was attempted.
Any error after the attempt is OutcomeUnknown and poisons FileStore. Explicit
reopen/inspection resolves durable state. No retry, replacement, cleanup or repair
occurs. Final checks validate the expected present/absent reference, quota, complete
current graph/inventory, original source descriptor and metadata ownership.

Native checks remain observations under cooperating immutable owners, not atomic
isolation against arbitrary same-user filesystem writes. Response loss may leave
a committed rename/removal. Staged uncommitted changes remain absent on recovery.
Private schema version1, original WAL/object bytes and fsync rules remain.
A reader borrow prevents mutable FileStore operations while that reader is used.
Native [quota administration](native-file-quota-administration.md) now uses scoped
global metadata CAS; content replacement, orphan reclamation, request idempotency,
common backup, Root/current-account integration and production gates remain open.
See [ADR0136](adr/0136-native-file-metadata-cas-and-logical-removal.md).

## Executed checks

Stable1.99 and minimum1.89 each passed565 affected checks:244 native files/storage/
CLI and321 database/WAL/transactions/backup. Five new regular cases and one new
compile-fail reader/mutation-lifetime case pass. The generated model tracks exact
revisions, visibility and physical charge through32 histories of1..31 operations,
with reopen after every step and both WAL versions. Eight same-byte inode
substitutions before/after commit verify rollback versus uncertain committed state.

Twenty-four new actual SIGKILL boundaries cover rename/removal, staged/committed/
caller-success, WAL1/2 and empty/8193-byte objects. Every recovery checks actual
WAL version/transaction, visibility/name/revision, physical bytes/charge and a
subsequent independent write. Existing31 native boundaries rerun, for55 native
kills per toolchain; lower-level core helper campaigns remain separate.

All594 frozen source/configuration hashes match. Formatting, strict workspace/fuzz
lint, explicit native-files/CLI/server builds and minimum all-fuzz compilation exit0.
The row codec/parser did not change; prior file_reference ASAN and unchanged-lockfile
advisory results belong to the preceding source. No new sanitizer campaign is claimed.
The separately completed immutable24444bc full1552-test stable/minimum matrices
certify that older source, not this later affected-source increment. See
[source-bound evidence](measurements/2026-10-10-file-mutations/verification.json).
