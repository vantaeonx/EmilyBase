# Explicit public admission metadata

This is a synchronous Rust catalog for a future user-only service. It implements
real durable operator decisions in the original private engine. It does not yet
expose a public sign-in/data route or remove the service-key requirement from any
existing HTTP endpoint. Use synthetic data; no production gate is complete.

The trusted owner can explicitly migrate private v4 to5 using
AccountStore.enable_public_admission_catalog or the current-service-key guarded
AccountRoot equivalent. The new singleton starts closed. A v1/v2/v3 store refuses;
activate its documented session clock and policy catalog explicitly first.
No store is upgraded merely by opening, inspecting, exporting or restarting it.
All v1..v4 formats remain readable. A previous reader refuses private v5.

The exact added table is:

| Column | Original type | Constraint |
| --- | --- | --- |
| id | INTEGER | singleton1, primary key |
| version | INTEGER | record version1 |
| enabled | BOOLEAN | false on migration |
| revision | TEXT | canonical positive u64, actual commit LSN |
| previous | TEXT | canonical u64, less than revision |

The complete v5 inventory contains eight exact tables. All columns are nonnullable.
Revision never exceeds the actual private committed prefix; zero, leading zeros,
signs, overflow, duplicate/missing singleton, wrong schema, extra table and unknown
record versions refuse even when the enclosing ordinary engine checksums are valid.
An enabled initial row with previous0 refuses. Existing project binding, users,
password verifiers, session references/clock and bounded policy fragments are still
validated completely. This is an internal private schema change, not a new page,
WAL, backup, registry or account-bundle version. No downgrade is implemented.

## Trusted native administration

Use the original retained root with the current service key loaded from a private
file. The following methods return PublicAdmissionReceipt metadata:

- enable_public_admission_catalog(project, key): explicit v4 migration or exact
  readonly v5 retry; an already open flag stays open.
- public_admission(project, key): readonly current metadata.
- set_public_admission(project, key, expected_revision, enabled): explicit CAS.

The receipt contains enabled, revision and previous. Native numeric fields are u64;
a future wire adapter must preserve their exact digits. Cloning/serializing these
values does not grant authority. Wrong/rotated/sibling service credentials refuse
before private work. Selected root identities and exclusive private owners remain
required. There is no new CLI/HTTP administration command in this increment.

For a real change, expected must match the current flag revision. An identical
request with current or immediately preceding revision returns the same receipt
without a commit. Earlier revisions conflict, including a stale reopening command
after an open/close cycle. Unrelated private commits do not replace this revision.
Original WAL capacity and write uncertainty apply. An exhausted revision refuses;
u64::MAX is reserved by the original WAL, while values beyond i64 remain exact.
Inspect uncertain state explicitly; do not retry blindly or assume rollback.

## Copy and reset

A trusted session-clock reset closes an open v5 flag while replacing session
incarnation/time in the same commit. Its flag revision is that commit and previous
is the former flag revision. A closed flag keeps its receipt during later resets.
Verified private restore and common-root restore perform this reset before the
new directory appears. They preserve passwords, account epochs, disabled state,
policy groups and service-key digests, and revoke old user sessions. Public rows
remain intact. A nonempty source can remain open with its old session still valid.

Generic low-level engine restore preserves metadata exactly and is not the verified
private reset protocol. Administrators must use the coordinated private/root restore
before admitting network users. Backup artifacts contain private metadata and
password verifiers; the flag is not an authenticated assertion of trust.

A future user-only gateway must check the current flag/session/policy inside the
retained private/data ownership scope. It must deny missing/closed admission and
missing policy, and recheck current state after network waits. Public signup, roles,
user SQL, HTTP routing, client SDK and dashboard integration are still pending.
[ADR0110](adr/0110-explicit-closed-public-admission-catalog.md) records this boundary.


Existing trusted HTTP/CLI policy-enable commands now report the actual current
private version,4 or5. A retry on v5 does not downgrade, close an enabled flag,
replace a policy or revoke a session. The native private_schema_version getter
returns only metadata; its Root wrapper requires the current project service key.


The subsequent [native admitted user gateway](native-public-user-gateway.md) now
checks current v5 admission for sign-in, refresh, logout, own-account metadata and
typed policy-enforced rows without a project service key. Closing suspends these
methods before time/password/public data work; an intentional reopen can resume a
current session. Verified copy/reset still revokes its old incarnation. These are
synchronous native methods; user-only HTTP/signup/roles/client integration remain
separate pending increments. Existing service-key APIs preserve their authority.
