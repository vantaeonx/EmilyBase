# Explicit v2 server-binary startup

Experimental native Linux startup on synthetic data. The ordinary Rust binary can
now explicitly select an existing v2 root, retaining the same original owners used
by library admission and complete backup. No format probing, migration, missing
component provisioning, session reset or implicit public admission occurs.

```sh
EMILYBASE_ACCOUNT_ROOT=./copies/restored \
EMILYBASE_ACCOUNT_LAYOUT=v2 \
EMILYBASE_MASTER_KEY_FILE=./private/master.key \
EMILYBASE_LISTEN=127.0.0.1:7000 \
emilybase-server
```

The root must already exist as a validated v2 restore. The master file must already
contain a valid private key under the existing secret-file rules. This example
contains no key and does not initialize a database. Keep backups outside the root.

## Exact selection before storage access

| Settings | Selection |
| --- | --- |
| No data setting, no layout | Existing default registry path behavior |
| Only EMILYBASE_DATA_DIR, no layout | Legacy registry mode |
| EMILYBASE_ACCOUNT_ROOT, layout omitted | Account v1 |
| Account root and exact `v1` | Account v1 |
| Account root and exact `v2` | Account/file v2 |
| Both data path settings | Refuse |
| Layout without an account root | Refuse |
| Empty/unknown/non-UTF-8 layout | Refuse |

Selection runs before secret loading or storage work. Values are compared exactly:
no trimming, case conversion, lossy decoding or fallback for invalid selectors.
Native paths remain OsString/PathBuf and may contain non-UTF-8 bytes. Invalid
configuration errors use static descriptions and do not echo supplied values.
V1 is still the default for account roots; it rejects v2. Explicit v2 rejects v1.
Neither mismatch creates or resets anything. Missing roots remain missing.

The selected synchronous root is opened on the existing blocking worker. The
network listener is bound only after admission. A busy listen port releases this
failed process's owners; a competing process cannot release another process's
original native/private/common ownership. Existing graceful shutdown behavior stays
unchanged. The shared network fixture now verifies signal9 for actual forced kills.

## Actual verified binary behavior

Eight new integration cases invoke the compiled emilybase-server binary and use
real local TCP. They cover empty/one/four-project roots, both WAL formats, original
owner exclusion, unchanged startup bytes, private session sign-in, master/sibling
key refusal for project SQL, acknowledged SQL writes, rollback, repeated restart,
six actual signal9 stops and scope/session preservation. Two kills occur with a
partial unacknowledged HTTP body; the incomplete request never becomes a row.

After restart, complete backup/verify/restore produces an independent copy. Its
previous source session is invalid, its data can change separately, and immutable
file bytes including orphans remain equal. This exercises native copy isolation,
not current user authorization for file operations.

Additional actual binary cases cover15 invalid/mismatched/missing mode settings,
busy file owners, a wrong manifest binding, corrupted WAL bytes without repair,
competing root admission, busy listen ports, native non-UTF-8 root names, private
master-file startup, explicit v1 compatibility and log confidentiality. Four pure
selection tests include exact defaults, conflict handling, non-UTF-8 values and
embedded-NUL selectors without invoking an OS environment containing NUL.

[ADR0166](adr/0166-explicit-binary-v2-startup.md) records the decision.
[Source-bound results](measurements/2026-10-19-v2-server-startup/verification.json)
record executed checks, hashes, counts and the scope of reused prior evidence.

## Containers and remaining gates

Both account Compose files pass EMILYBASE_ACCOUNT_LAYOUT, defaulting to v1 only
when the variable is unset. An explicitly empty value stays invalid. A preexisting
v2 root may be selected by setting the variable to v2. The default registry Compose
file and default image target retain their existing mode.

These Compose changes have not been exercised in a local container: Docker is not
installed in this environment. Native TCP tests are not a container/deployment or
upgrade drill. V2 container recovery, current file permissions, roles, aggregate
network admission, realtime, dashboard, Kotlin, encrypted backups and production
acceptance remain open. Never attach real user data based on this increment.
