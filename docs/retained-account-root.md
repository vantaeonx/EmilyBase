# Synchronous retained account root

`AccountRoot::open(path, shared_password_pool)` opens an existing selected root
and moves the exact fully validated registry/private owners into a synchronous
service. There is no close/reopen gap. It creates no missing root, migrates no
schema, advances no clock and resets no session. Ordinary reopening preserves
valid sessions; explicit restore continues to reset their scope before publication.

The first service admits at most four declared private stores, checking the count
before opening a registry or account database. Offline bundle tools keep their
128-store cap. Temporary common validation still reconstructs sensitive archive
images and decoded models; this count does not reserve their combined heap.

Project and private rosters stay fixed for this owner. `projects` reports trusted
operator metadata; `rotate_project_key` is an explicit trusted operator action.
There is no dynamic create/attach operation. Private operations require the current
project service key: `create_user`, `sign_in`, `refresh_session`, `logout_session`,
`set_disabled`, `change_password` and `with_access`. A generic project without a
declared private store keeps its service-key SQL path but private operations refuse
without implicitly provisioning directories.

Root, private container/store, registry/data and exact manifest identities are
checked before operations. The root lock outlives registry/private children on
drop. Filesystem ancestors and local operator configuration remain trusted; this
is not protection against an unrestricted malicious administrator.

Callers supply trusted bounded service Unix time, never a client-provided time.
Lower time is refused across restarts. An authorized forward credential attempt
can durably advance the clock even if its credential is denied. Stale project
keys fail before private credential/time work. Refresh remains atomic and single
use; storage uncertainty requires inspection/reauthentication, never blind retry.
Logout requires the current refresh credential. Password/disable epoch changes
invalidate historical families. Project-key rotation changes the service gate
without silently resetting user families.

`with_access` invokes an immediate callback with the borrowed current principal.
It cannot return that borrowed proof as a detached authorization object; copied
account metadata grants no later permission. `execute` retains existing service-key
SQL authority only. Access/refresh tokens are never accepted as SQL keys, and the
private database is not reachable through public table names.

The separate [HTTP transport and explicit executable mode](private-http.md)
supply body/worker/rate admission and explicit secret DTOs. Public signup policy,
roles/row policies, automatic authoritative catalogs and whole-process memory
admission remain pending. Synthetic-only experimental status is unchanged.

See [ADR0080](adr/0080-retained-private-root-service.md), the
[root restore contract](account-root-restore.md) and [testing](testing.md).
