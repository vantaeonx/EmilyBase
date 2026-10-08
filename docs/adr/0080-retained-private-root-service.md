# ADR0080: Retained owners for synchronous private root services

Status: accepted for experimental synchronous services; network/production gates remain open.

Open an existing restored root without creating paths, resetting sessions,
advancing clocks or migrating schemas. Reuse complete common-boundary inspection
and retain the same registry/private owners after validation. A close/reopen gap
between inspection and service construction would discard that ownership proof.

The initial service admits at most four explicitly declared private stores,
before opening any registry/account store. Offline archives retain their 128-store
bound. This is an active-owner count, not a combined model/cache/transient heap
reservation. The service keeps the exact selected manifest and fixed project
inventory; dynamic project/private attachment requires a separate durable protocol.

Validate selected root, private container/store and manifest identities before
each operation. Require the current project service API key before any private
credential/time operation. Expose real synchronous provisioning, sign-in, refresh,
logout and borrowed access checks; session tokens do not authorize public SQL.
Trusted callers supply service time. A normal restart preserves scopes, sessions
and time floors; invalid/backward time is refused by the existing durable lifecycle.

The owner is intentionally non-cloneable. Verified principals are consumed by an
immediate callback while the private owner is borrowed; returned account metadata
is not a reusable authorization proof. Rotation of a project service key changes
that gate without silently revoking user families. Trusted credential changes
continue to invalidate sessions through the existing epoch/disabled checks.

HTTP admission, workers/rate limits, transport DTOs, public signup, roles/RLS,
dynamic authoritative service catalogs, whole-process memory admission and
production acceptance remain separate work. No network account route is enabled
by this synchronous service increment.
