# Experimental account bundle format 1

EMILYBND-1 contains the complete existing EMILYREG-1 image plus an explicit roster
of private account EMILYBAK-1 images. Public source code never publishes database
contents. Bundle bytes contain plaintext data, key/password/session verifiers and
private metadata; keep them outside Git and public endpoints. Use the ignored .account-bundle extension for local output. No encryption,
signature, full platform discovery or restore publication is implied.

All integers are unsigned little-endian. Maximum whole-image size is128 MiB.
Unknown versions, nonzero reserved bytes, inexact lengths, duplicate project or
nested database identities, foreign scope and incomplete private schemas fail.

| Header offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII EMILYBND |
| 8 | 2 | version1 |
| 10 | 2 | header size128 |
| 12 | 4 | private roster count0..128 |
| 16 | 8 | complete EMILYREG image byte length |
| 24 | 8 | payload byte length, exactly whole image minus128 |
| 32 | 32 | SHA-256 of the payload |
| 64 | 60 | zero reserved bytes |
| 124 | 4 | CRC32 of header bytes0..123 |

The payload begins with the exact registry image including its header. Registry
length is128..128 MiB, subject to the enclosing whole-image cap. It is validated
with all existing registry canonical metadata, project order and nested WAL rules.
The following private entries are sorted strictly by project ID:

| Entry offset | Bytes | Meaning |
| --- | --- | --- |
| 0 | 32 | lowercase ASCII-hex project ID present in the registry |
| 32 | 8 | complete private EMILYBAK image byte length |
| 40 | image length | exact private archive including its header |

Private lengths use the existing single-database bound. Private versions1/2/3/4
and WAL1/2 are validated completely against each independently expected project.
No public data database or two private databases may share a database identity.
Missing private entries are permitted: the report counts only the supplied roster,
including an empty roster. It does not discover or certify absent stores.
SHA-256/CRC detect corruption; a recomputed valid bundle is neither authenticated
nor evidence that an external producer used a common capture boundary.

ProjectStore::capture_account_bundle accepts already exclusively owned private
AccountStores and a mutable registry. Outstanding authorized project capabilities
are refused. All project data directory/WAL owners are acquired before the first
acknowledged prefix and retained through private capture, complete validation and
final registry source checks. Private owners remain with the caller on success
and error. The unchanged registry backup_image uses the same scoped capture.
No token is issued, epoch advanced or credential invalidated by inspection/capture.

The combined byte count is checked before retaining each next private image and
before output allocation. The next individual archive must still be materialized;
encoded size is not a whole-process memory reservation. Opening/replay, retained
models, temporary WAL images and output copies have distinct resource costs.

This is a trusted offline library API. backup_account_bundle publishes a private
0600 file outside the registry through descriptor readback, full validation,
file/parent fsync and no-replace rename. inspect_account_bundle reads only private
regular single-link bounded files, refusing final symlinks/FIFOs/broad modes.
Post-rename errors report publication uncertainty and preserve the selection.
Choose a separate private backup directory; paths/ancestors remain operator-trusted.

The real CLI verifies an existing archive without printing private contents:

```sh
cargo run --locked -p emilybase-cli -- account-bundle-verify ./synthetic.account-bundle
```

Its aggregate counts do not establish permission, full private-store discovery or
common-capture provenance of third-party bytes. Verification changes no live state.
The command does not create or restore bundles. See
[ADR0076](adr/0076-owned-account-bundle-file-publication.md).

Combined root restore with mandatory private session reset, authoritative private roster,
private worker admission, HTTP authentication, roles and row policies remain open.
Existing EMILYREG/EMILYBAK and database/WAL formats are unchanged; no silent upgrade
or PostgreSQL compatibility is promised. See
[ADR0075](adr/0075-common-registry-private-capture.md).

Explicit private v4 catalogs are opaque nested original archives in this unchanged
bundle version. Capture/inspection checks their complete header/chunk inventory.
Root restore preserves policy groups/revisions and resets only private session
incarnation/time; old user credentials remain revoked. See [catalog compatibility](policy-catalog.md).
