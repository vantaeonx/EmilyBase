# Private master-key files

The experimental Linux server accepts exactly one configured master source:
EMILYBASE_MASTER_KEY (the existing exact environment value) or
EMILYBASE_MASTER_KEY_FILE (a local path). Supplying both, even an empty value,
refuses before data directories are opened. Missing/invalid input refuses without
logging the key, selected path or raw configuration. Both legacy registry mode and
explicit private-root mode use this startup loader.

A file contains exactly64 lowercase hexadecimal characters, optionally followed
by one LF. No trimming, CRLF, BOM, uppercase, spaces, extra newline or invalid UTF-8
is accepted. Environment values keep their original exact64-character policy.
Generate keys independently from an OS random source; never reuse the synthetic
values from tests. No new stored database, session, verifier or archive format is
introduced.

The selected file must be regular, readable by the service process, singly linked
and have exact permission bits0400 or0600. Group/world access, executable/special
bits, hard-link aliases, final-component symlinks and nonregular files refuse.
Its owner need not equal the process UID; the service must have actual read access.
Trusted operators control the parent directories and ancestor traversal. The
loader does not sandbox those ancestors or defend against a malicious filesystem
or administrator able to alter process memory and metadata.

Open uses NOFOLLOW, NONBLOCK and CLOEXEC. Validate descriptor metadata before
allocating; read at most66 bytes. Compare the selected name and retained descriptor
identities, length, mode and modification/change timestamps after the read. An
observed replacement or metadata/content change refuses. This is bounded startup
configuration loading, not a durable secret store or a continuous file watcher.
Reads run on a blocking worker before router/data initialization.

Owned key/read buffers are zeroized on release. This does not erase the source
file, environment, OS caches, allocator history, transport buffers or operator
copies. The on-disk file remains plaintext. Protect and back it up separately;
secret encryption and an external secret-manager integration remain future work.
The master secret is operator configuration, excluded from account-root bundles.
Keep its file outside the strict selected account-root inventory.

## Native service

Privately provision the key outside the repository with mode0400/0600 and readable
by the service account. Set EMILYBASE_MASTER_KEY_FILE to that path and remove
EMILYBASE_MASTER_KEY from the service environment. Select only one documented data
mode; the loader never initializes a root, fixes file permissions or creates a key.

The master digest is loaded once. Replacing the file while the server runs does
not rotate the live key. For deliberate replacement: stop the server, privately
publish and verify the new file, restart and verify authentication. Preserve
existing project service keys and data. This is a controlled-restart operation;
there is no master-key rotation endpoint or automatic retry of failed publication.

## Separate container variant

Use compose.accounts.file.yaml on its own, with a stable separate Compose project
name. It reuses the accounts image and the existing unprivileged restrictions.
The server container receives only a path, never EMILYBASE_MASTER_KEY. A trusted
Docker operator can still read its volume; this is not encryption or a security
boundary against that operator. Never publish expanded inspection output.

This example creates a new disposable synthetic deployment. The explicit one-off
process writes stdin as UID10001 with noclobber and umask077. No key is passed as
a command argument. An existing master.key causes refusal; inspect a partial or
uncertain file privately rather than overwriting it or retrying blindly.

```sh
docker compose -f compose.accounts.file.yaml -p emilybase-private-file build
od -An -N32 -tx1 /dev/urandom | tr -d ' \n' |
  docker compose -f compose.accounts.file.yaml -p emilybase-private-file \
  run --rm --no-deps -T --entrypoint /bin/sh server \
  -c 'set -C; umask 077; cat > /var/lib/emilybase/master.key'
docker compose -f compose.accounts.file.yaml -p emilybase-private-file \
  run --rm --no-deps --entrypoint emilybase server \
  account-root-init /var/lib/emilybase/account-root \
  --name synthetic-private-project --reset-at 0
docker compose -f compose.accounts.file.yaml -p emilybase-private-file \
  run --rm --no-deps --entrypoint emilybase server \
  account-root-verify /var/lib/emilybase/account-root
docker compose -f compose.accounts.file.yaml -p emilybase-private-file up --detach --wait
```

Retain this volume/name and the separately protected key for subsequent starts.
Existing environment-based deployments are not automatically migrated to it.
Backup/restore commands from [deployment](deployment.md#private-root-container)
apply with this file/name; restored session scope changes, the operator file does
not enter the bundle. Different examples still share host port7000 unless changed.

## Verification boundaries

Unit/property cases check exact content, permissions, links, Unicode paths,
deterministic selected-file replacement and metadata changes. Actual process
cases check both data modes refuse before creation, a FIFO exits before a deadline,
and controlled-restart replacement preserves project metadata and committed WAL.
The common native/container lifecycle runs with both environment and file keys,
including refresh/password/disable/enable/prune/logout ACK kills and verified
restore. Local native runs do not prove container/cgroup enforcement; hosted CI
runs the file variant against an actual image and inspects its environment.
See [ADR0088](adr/0088-private-master-key-file.md) and [testing](testing.md).
