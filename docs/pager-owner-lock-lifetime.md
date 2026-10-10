# Pager owner lock lifetime

Pager previously relied on closing its private File to release the kernel lock.
A duplicate open file description can outlive that File, including during native
process creation. An exact regression duplicates the internal descriptor, drops
the public Pager owner and tries to reopen: the original implementation returns
Busy deterministically. The duplicate is a test model of an inherited description,
not a second authorized Pager, and uses no unsafe fork or pre-exec hooks.

Linux associates flock locks with open file descriptions and releases them on
explicit unlock or closure of all duplicates; see
[flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html). The earlier publisher
test interference was consistent with that lifetime, but its actual syscall
interleaving was not traced. This new deterministic model establishes the Pager
lifetime defect separately from that scheduling inference.

## Explicit private ownership

A private LockedFile now owns the successfully acquired Pager lock and issues
unlock when dropped. It is created immediately after lock acquisition. Opening
retains it across every length/header admission error; creation retains it across
write/readback/selection/synchronization errors. Successful Pager construction
moves that same guard into the returned owner. Private File duplication occurs
before acquiring the creation lock, still before any irreversible selection and
without adding a descriptor to the original successful path.

Active owners still refuse competing opens. Ending the owner no longer relies on
an unexposed inherited descriptor disappearing first. Closing that old duplicate
after a separately opened successor has acquired its lock cannot unlock the
successor's independent description. Drop is best-effort: unlock errors cannot be
reported from a destructor, and ordinary File closure remains the fallback. This
does not grant the private duplicate a second Pager or stale write authority.

No raw descriptor/clone API is exposed. Public create/open/read/write signatures,
private owned staging, exact header/page readback, no-replace/parent fsync,
PublicationUnknown, page bytes/version and poisoned-write refusal remain. The
creation duplicate moves earlier than its old preselection point; descriptor
allocation failure remains prepublication. No transaction/WAL ACK or async layer
changes. WAL/directory owners have their own separate ownership implementations;
this change does not silently replace them or qualify their lifetime contracts.

See [ADR0133](adr/0133-explicit-pager-file-lock-owner.md).

The existing WAL owned-file API intentionally retains a lock until the last
caller-supplied clone closes, as its
[owned-file contract test](../crates/wal/tests/owned_file.rs) explicitly requires.
Pager has no public File constructor/export and its duplicates are internal.
That distinct WAL behavior remains unchanged and is covered by the affected-core
checks; this Pager correction is not a reason to silently unify those contracts.

## Executed evidence

The original owner-lifetime regression exited101 with0 passed/1 failed and Busy.
After correction stable1.99 and minimum1.89 each passed534 affected checks:
213 storage/native/CLI and321 database/WAL/transactions/backup. Four new regular
cases cover created/opened owners and successors, four failed length/header opens
with retained duplicates, four original before/after file/parent synchronization
outcomes, and poisoned-write refusal with exact later page writes. Nine affected-
core ignored fixtures are explicitly invoked by their parent tests; they are not
additional top-level passes. Existing native25 kill boundaries reran; no new kill.

All583 frozen hashes match. Formatting, strict stable workspace/fuzz lint, CLI/server
builds and minimum all-fuzz checks exit0. The preserved WAL last-caller-clone contract
passes. No parser/format change or new filesystem sanitizer result is claimed.
See [source-bound evidence](measurements/2026-10-10-pager-owner/verification.json).
The separate24444bc complete workspace run remains pending and belongs to an older
source;534 is an affected-source matrix, not a full current workspace result.
