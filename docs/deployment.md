# Experimental local containers

The image contains the compiled original Rust server and CLI. It requires no
external database engine, paid service, JavaScript runtime or Python runtime.
Node and Python are developer tools for optional client/integration checks.
This deployment is for disposable synthetic data on Linux local filesystems.

## Start

Default Compose selects legacy registry mode. The separate
[private-root configuration](#private-root-container) selects an explicitly
initialized account root. Never set both data variables or merge the two files.

Install Docker Engine with its Compose/build plugins using the
[official instructions](https://docs.docker.com/engine/install/ubuntu/).
Rootless Docker works with cgroup v2/systemd delegation; see
[its resource-limit requirements](https://docs.docker.com/engine/security/rootless/tips/#limiting-resources).

From this repository, supply a random secret privately and build:

```sh
export EMILYBASE_MASTER_KEY="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
docker compose build
docker compose up --detach --wait
curl --fail http://127.0.0.1:7000/health
```

The key is required, exactly 64 lowercase hexadecimal characters. Keep it in a
private secret manager or private operator configuration outside the checkout;
the example's generated value is not saved. Retain the same key before restarting.
Compose currently passes it as environment configuration: the trusted Docker
operator can inspect it. Secret-file loading and secret encryption are pending.
Avoid publishing expanded Compose configuration or container inspection output.
Never put the key in a URL, public frontend code, Git or logs.

The host listens only on `127.0.0.1:7000`; `EMILYBASE_PORT` can change that port.
The server binds all interfaces inside its own container. A named `data` volume
stores `/var/lib/emilybase`; individual projects live under its private `projects`
directory. Keep the Compose project name stable so the same volume is selected.
No host checkout/data directory is bind-mounted into the container.

The runtime process uses UID/GID 10001, a read-only image filesystem, all Linux
capabilities dropped and `no-new-privileges`. The writable volume is private;
`/tmp` is a bounded 16 MiB temporary filesystem. Compose configures 512 MiB memory,
one CPU and 64 processes. Verify actual cgroup enforcement on the deployment host;
rootless engines without delegated controllers may ignore those settings.

Health is a public liveness response, not a database readiness check. A damaged
project in legacy registry mode can return503 while health and healthy projects
remain available. Explicit account-root startup validates its complete declared
roster and refuses before listening if any private store is corrupt.
There is no automatic restart loop: inspect failures before restarting.
SIGTERM drains accepted work; Compose allows 30 seconds before forced termination.
A commit can succeed without an observed response: inspect transaction state
rather than retrying automatically. See the [HTTP contract](server.md).

## Private-root container

Use compose.accounts.yaml on its own. Its accounts image target sets only
EMILYBASE_ACCOUNT_ROOT; the default registry image keeps EMILYBASE_DATA_DIR.
The separate default Compose name is emilybase-private, so this example selects a
new volume. Retain your chosen project name for later restarts. The master key
must already be supplied privately as above; no key or password is a CLI argument.

```sh
docker compose -f compose.accounts.yaml -p emilybase-private build
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --no-deps \
  --entrypoint emilybase server account-root-init /var/lib/emilybase/account-root \
  --name synthetic-private-project --reset-at 0
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --no-deps \
  --entrypoint emilybase server account-root-verify /var/lib/emilybase/account-root
docker compose -f compose.accounts.yaml -p emilybase-private up --detach --wait
```

Zero is the explicitly selected initial floor for a new empty synthetic root.
The CLI refuses any existing target; server startup never initializes, restores or
resets it. Obtain the generated project ID through authenticated operator listing
and rotate its first usable service key. Provision users/sign-in only from a
trusted backend holding that service key; see [private HTTP](private-http.md).
The same unprivileged/read-only/loopback/resource restrictions apply. Both examples
use host port7000: stop the other deployment or choose a different EMILYBASE_PORT.

For a common public/private backup, stop the server and use the root commands.
The backup destination is outside the strict root inventory. Choose new names:

```sh
docker compose -f compose.accounts.yaml -p emilybase-private stop
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --no-deps \
  --entrypoint emilybase server account-root-backup /var/lib/emilybase/account-root \
  /var/lib/emilybase/account.backup
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --no-deps \
  --entrypoint emilybase server account-bundle-verify /var/lib/emilybase/account.backup
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --no-deps \
  --entrypoint emilybase server account-bundle-restore /var/lib/emilybase/account.backup \
  /var/lib/emilybase/account-copy --reset-at "$(date +%s)"
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --no-deps \
  --entrypoint emilybase server account-root-verify /var/lib/emilybase/account-copy
```

Select the trusted restore time deliberately. Start the independent copy while
source remains stopped, check known synthetic rows and sign in again:

```sh
docker compose -f compose.accounts.yaml -p emilybase-private run --rm --service-ports \
  --env EMILYBASE_ACCOUNT_ROOT=/var/lib/emilybase/account-copy server
```

Restore preserves project service keys but changes private session scope before
selection, so source access/refresh tokens cannot authorize the copy. Exit the
copy server and resume the source with the same Compose up command. A backup in
this volume alone does not protect against volume loss; privately copy and verify
it on independent storage. Never manually swap selected files or run both servers
against one root. Failed/uncertain publications require inspection, not auto-retry.
[ADR0085](adr/0085-explicit-private-root-container.md) records this adapter.

## Project backup and restore

Stop the server before offline CLI maintenance. Obtain the server-issued project
ID through the administrator API; never substitute a display name into a path.
Use only this documented 32-character lowercase hexadecimal ID:

```sh
project_id='replace-with-server-issued-id'
case "$project_id" in
  *[!0-9a-f]*|'') exit 1 ;;
esac
[ "${#project_id}" -eq 32 ] || exit 1
docker compose stop
docker compose run --rm --no-deps --entrypoint emilybase server \
  backup "/var/lib/emilybase/projects/$project_id/data" /var/lib/emilybase/project.backup
docker compose run --rm --no-deps --entrypoint emilybase server \
  backup-verify /var/lib/emilybase/project.backup
docker compose run --rm --no-deps --entrypoint emilybase server \
  restore /var/lib/emilybase/project.backup /var/lib/emilybase/restored-data
docker compose run --rm --no-deps --entrypoint emilybase server \
  sql /var/lib/emilybase/restored-data 'SELECT * FROM items LIMIT 10'
docker compose up --detach --wait
```

Replace the example query with a known synthetic table. Existing archives and
restore destinations are preserved; choose a new name for the next backup.
The restored database is a separate offline managed directory. It is not
automatically adopted as a project. Archives contain plaintext committed WAL,
not project names/key metadata or an administrator secret. The separate
whole-registry format below includes project names/key digests. An archive kept only in the source volume
does not protect against loss of that volume: copy it to private independent
storage and verify the copy using the CLI. No real data is authorized yet.

Do not put backup archives or restored directories under the registry's `projects`
root: unknown registry entries correctly prevent startup. Do not copy a live WAL
as a backup. Do not replace existing project files with restored data manually.
See [backup verification and limits](backup-format.md).

## Whole-registry backup and independent restore

Stop the server. These commands preserve current project IDs, names, rotated key
digests/epochs and every committed database history. They never overwrite paths:

```sh
docker compose stop
docker compose run --rm --no-deps --entrypoint emilybase server \
  projects-backup /var/lib/emilybase/projects /var/lib/emilybase/registry.backup
docker compose run --rm --no-deps --entrypoint emilybase server \
  projects-backup-verify /var/lib/emilybase/registry.backup
docker compose run --rm --no-deps --entrypoint emilybase server \
  projects-restore /var/lib/emilybase/registry.backup /var/lib/emilybase/restored-projects
```

For a disposable check, serve the independent registry while the original server
remains stopped. The Compose environment supplies the external master credential:

```sh
docker compose run --rm --service-ports \
  --env EMILYBASE_DATA_DIR=/var/lib/emilybase/restored-projects server
```

Check the scoped HTTP queries with privately retained current project keys.
Restored credentials remain valid until rotated; administrator rotation on a copy
does not change the original. Exit the temporary server, then use `docker compose
up --detach --wait` to resume the original registry. The real container probe
executes this restored-service check. Copy/verify archives on independent private
storage; a backup in the original volume alone cannot survive that volume's loss.
See [archive format and limits](registry-backup-format.md). Future object/session
storage, unrelated standalone indexes, streaming/encryption and stable upgrades
remain outside this currently implemented registry backup.

## Image replacement and compatibility

Current readers accept storage version 1 and WAL versions 1/2. Opening is never a
format upgrade. Explicit offline `compact` writes a WAL-2 baseline; version-1-only
older readers reject it. Check [format rules](file-format.md) and the relevant
release notes before choosing a different revision. There is no stable production
release or verified arbitrary cross-version downgrade procedure.

For a candidate revision, use a separate disposable volume first. After a verified
offline backup and independent restore check, preserve the current image/revision,
stop the server, rebuild the new image, and recreate using the same Compose project
and volume. Check administrator project listing, each project's transaction state
and expected synthetic rows before accepting writes:

```sh
docker compose stop
docker compose build
docker compose up --detach --no-build --force-recreate --wait
```

The executed container probe verifies same-revision recreation with intact data,
keys and IDs, including explicit WAL compaction. It does not prove compatibility
with a future file-format change. Future migration acceptance must test old/new
readers, restored backups and rollback before claiming a safe upgrade.

`docker compose down` retains named volumes. `down --volumes` destroys their data;
the test probe uses that flag only for its own randomly named synthetic project.
Do not use it to maintain a deployment you intend to keep.

## Executable container probe

```sh
python3 tests/containers.py
```

It builds the image, allocates a random Compose project and loopback port, and
creates only synthetic data. It verifies non-root execution, applied cgroup-v2
memory/CPU/process limits, read-only root, private directories, scoped keys,
literal parameters, failed-script rollback and key rotation. Offline backup,
independent replay/restore, no-clobber destinations, new restored writes, WAL-2
compaction and same-volume container recreation run through the actual CLI.
The compiled standalone index CLI also creates, inserts, reads, deletes and verifies
a separate synthetic index; table/WAL index integration remains pending.
Whole-registry archive/verify/restore and an independently served copy also check
retained scoped credentials, isolated rows and new writes without modifying source.

A real SIGKILL writer check restores every fully received SQL response and a
gapless whole-script prefix. Complete commits whose responses were lost may also
survive. Deliberate journal damage returns 503 for that project while its sibling
continues to serve. Captured logs must exclude the probe's credentials, IDs and
SQL. The probe finally removes only its own containers/volume, including on error.
It respects `DOCKER_HOST`, `DOCKER_CONFIG` and optional `EMILYBASE_DOCKER`.

For the already installed/compiled project SDK, add `--sdk`; `--no-build` reuses
`emilybase:local`. The SDK's native-process restart case is skipped in external
container mode; the Python probe owns and verifies container restart itself.
No browser UI, TLS, load test, future platform-object backup, physical power-loss experiment
or full security audit is implied by these checks.


The separate private-root probe is:

```sh
python3 tests/account_containers.py
```

It builds the accounts target and uses its own random synthetic volume. It checks
one-mode image configuration, non-root restrictions and the common root lifecycle:
first initialization, operator rotation, sessions, two-request refresh race, five
received-ACK SIGKILLs, password/disable/re-enable epochs, WAL2 compaction, common
backup, no-clobber restore, clone/source separation, logout durability and corrupt
private WAL startup refusal without automatic repair. All collected logs are
screened for plaintext and JSON-escaped credentials/identifiers. Cleanup removes
only the probe's own disposable instances and volume. The hosted container job
runs both probes. --no-build reuses emilybase:accounts-local.

An explicitly separate --native preflight uses compiled local Rust binaries and
the same HTTP/CLI lifecycle, but does not execute or claim Docker/cgroup checks.
Those native checks passed on stable/minimum Rust on the development host; actual
new container execution is pending its first hosted run. No production gate closes.
