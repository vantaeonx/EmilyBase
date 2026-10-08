# Offline registry and private account root restoration

## Operator cycle

For an existing synthetic private bundle and stopped source services:

```sh
cargo run --locked -p emilybase-cli -- account-bundle-verify synthetic.account-bundle
cargo run --locked -p emilybase-cli -- account-bundle-restore synthetic.account-bundle restored-root --reset-at 50
cargo run --locked -p emilybase-cli -- account-root-verify restored-root
cargo run --locked -p emilybase-cli -- account-root-backup restored-root second.account-bundle
cargo run --locked -p emilybase-cli -- account-bundle-restore second.account-bundle independent-root --reset-at 50
```

The two example destination directories are ignored by Git; operator storage should
remain outside the source checkout or in an explicitly ignored runtime directory.

Here 50 is an explicit synthetic Unix clock value. Operators must supply their
trusted reset time; the CLI does not derive it from the archive or use an implicit
clock. Only decimal values from 0 through the largest signed 64-bit integer are
accepted. Invalid time fails before reads/staging without echoing its supplied
text, including a hyphen-prefixed value. CLI output contains aggregate counts and
archive size/reset time only, without IDs, names, credentials or row values.

`account-root-backup` captures exactly the root manifest's declared private roster
and every current registry project. Later committed data/user/session changes are
included; capture never resets sessions, advances clocks or rotates keys. The
source is exclusively locked through common data/private prefix capture and final
inventory checks. Concurrent output publication can happen after separate captures;
busy source ownership is refused without retries or weakening locks. Targets inside
the source root, unknown/missing private entries and existing destinations fail.

The library equivalents are `capture_account_bundle_root` (sensitive bytes) and
`backup_account_bundle_root` (private file). They reuse complete root inspection and
the owned no-replace file publisher. Final aggregate input size is bounded; copies
and output retention are still outside a whole-process heap reservation contract.

These commands consume an existing bundle/restored root. They do not bootstrap a
new HTTP account service or discover separately attached private stores. See
[ADR0079](adr/0079-offline-root-operator-cycle.md).

## Restoration and inspection protocol

Experimental library APIs restore a verified EMILYBND image into a new directory:
`restore_account_bundle(path, target, pool, now)` and
`restore_account_bundle_bytes(bytes, target, pool, now)`. The image and target paths
and reset time are trusted local operator inputs. Sources are sensitive private
files; no key, password, session token or row is logged or returned in reports.

The selected layout is:

```text
restored-root/
  root.json
  registry/<project-id>/project.json
  registry/<project-id>/data/redo.wal
  private/<project-id>/redo.wal
```

Only explicitly bundled private stores appear under `private`. A bundle may have
an empty/subset private roster, including an empty registry. This does not discover
or certify all services of an existing deployment. Existing targets are never
overwritten. API key digests and epochs are preserved; rotate keys explicitly if
old project credentials must be invalidated.

Complete bounded source validation precedes staging. The restorer owns and locks a
0700 staging directory, installs the exact registry and prepares every private
store there. Each private version1/2/3 becomes version3 with a fresh session
incarnation and the trusted clock floor. Passwords/account epochs/disabled state
remain intact; historical access and refresh tokens fail after restoration. Every
private reset must fit its WAL limit. There is no implicit history compaction.

After preparation, all private owners are acquired before any data prefix is
captured. Every data owner is held through private validation and final path checks.
Exact registry bytes and SHA-256 hashes of the prepared private archives must match
the expected installation. Counts or database IDs alone are insufficient. Root,
private container, private store and registry owners are checked, as are the exact
canonical manifest and declared directory entries. Input archive bytes are never
written as intermediate credential files.

The manifest is a canonical JSON/CRC envelope with a version1 payload containing
`private_projects` (at most128 sorted unique32-character lowercase hexadecimal
identifiers) and `reset_at` (at most the largest signed 64-bit integer). Total
encoding is at most8192 bytes. Alternate encoding, unknown/duplicate fields,
trailing bytes, invalid identifier syntax/order/version/time and checksum damage
are rejected. The full root inspector additionally checks registry membership.
Manifest contents describe layout and never grant access or prove provenance.

Private WALs, checkpoints, child directories, manifest and root are synchronized
before no-replace root selection. The owned destination parent is synchronized and
checked before success. Errors after root rename report publication uncertainty;
the selected root is retained for verification. Child integrity/identity failure
retains the entire suspect stage to preserve foreign entries. Owned ordinary
pre-publication fault stages are cleaned. Killed-process or retained suspect stages
are never automatically adopted or swept; use an independent new destination to retry.

`inspect_account_bundle_root(path, pool)` performs offline full engine/private and
layout validation while holding owners. It creates no missing root and does not
authenticate or reset accounts. Current private clocks must be at least the
recorded reset floor; normal later user/session operations are allowed. The pure
`inspect_account_bundle_root_manifest_bytes` only checks bounded canonical metadata.
Neither inspector establishes capture provenance, permissions for a remote caller,
or the absence of services outside this layout.

The numeric128MiB bundle-image limit also bounds final aggregate validation. Reset
adds journal records, so sources near WAL or aggregate image limits may be refused
before selection. These are format/count limits, not a whole-process heap quota;
retained decoded snapshots, caches and transient allocations remain separate gates.

[ADR0078](adr/0078-atomic-account-bundle-root-restore.md) records this layout decision.
Linux descriptor-owned paths and trusted local ancestors remain assumptions. A
malicious administrator with unrestricted file write access is outside that model.
Automatic HTTP attachment, authoritative service roster, roles/RLS, whole-process
resource admission, hardware power loss and production/security acceptance remain open.
