# Offline policy administration

The Rust CLI operates on an existing private account root with its current project
service key. Stop the server first. All operations retain the original exclusive
root/data/private owners; a live owner refuses instead of opening a second writer.
Use synthetic data while production gates remain open.

Provision the service key privately in a separate regular file outside the root
and repository. It must contain exactly64 lowercase hexadecimal characters,
optionally one LF, and have exact0400 or0600 permissions and one hard link. The
file must be readable by the operator. Final symlinks, FIFOs, broad permissions,
extra whitespace/newlines, uppercase and changed metadata refuse. Parent directory
control belongs to the trusted operator. This is the shared bounded loader used
by [master-key startup](master-key-files.md), not encryption or live key rotation.

Pass only its path as `--key-file`; the CLI accepts no service-key value argument
or environment fallback. Initialization does not print usable credentials: obtain
a current service key through existing authenticated operator rotation, then store
it privately. Never copy sample/test keys into a deployment.

```sh
emilybase account-policy private-root PROJECT_ID --key-file service.key enable
emilybase account-policy private-root PROJECT_ID --key-file service.key list
emilybase account-policy private-root PROJECT_ID --key-file service.key \
  install items --expected 0 < policy.json
```

Replace PROJECT_ID with trusted current project metadata. Enable explicitly
migrates private v3 to v4 and is idempotent. It changes neither the session clock
nor existing users/session incarnation. Without enable, list/install refuse.

Install reads the exact original UTF-8 policy document, at most16,384 bytes, from
stdin. It preserves whitespace for checksums; the final newline is part of this
policy definition. Unknown/duplicate fields and unsupported grammar refuse. The
[documented policy rules](row-policies.md) must compile against the actual table
schema; callers cannot supply a table ID, schema, user identity or trusted time.

`--expected` is required and accepts canonical u64 decimal digits only. Use0 for
no policy bound to the actual current table ID, otherwise the current revision.
Exact already-installed bytes/schema with the current or recorded predecessor
expectation return the existing receipt without writing. Changed stale content
refuses. Dropping/recreating a table gives a new identity and requires expectation0;
obsolete bounded entries are retained. There is no automatic retirement/reclaim.

List returns `{"policies":[...]}`; install returns `{"receipt":...}`. Receipt
`table`, `revision`, `previous` are exact decimal strings, including values above
the JavaScript integer range. `sha256` has64 lowercase hex digits. Enable returns
`{"private_version":4}`. Output is limited to65,536 bytes and contains metadata,
never definitions or credentials. Status failures use nonzero exit codes and static
diagnostics. The root's native typed validation/atomicity is unchanged.

Input validation finishes before root acquisition. A pipe can wait indefinitely
for operator input; this local workflow has no network body deadline. It stops at
limit+1 bytes rather than waiting for an oversized stream to close. Current key
verification occurs after complete input, so rotation during a pipe wait rejects
the previously loaded key. A lost/failing stdout write can follow a durable commit:
inspect/list and explicitly retry only the original exact definition/expectation
when its current/predecessor contract still applies. Never assume rollback from a
missing receipt. There is no automatic retry or idempotency history.

Verified root backup/restore preserves policies and usable service keys while
resetting session incarnation. Old user sessions refuse in the copy; fresh sign-in
is required. Key files are external configuration and do not enter root bundles.
See [HTTP policy administration](policy-administration.md), [v4 catalog](policy-catalog.md)
and [ADR0107](adr/0107-offline-service-policy-cli.md).

This interface grants trusted service-operator policy authority. Public user-only
admission, roles, dashboard, resource/load/security/upgrade and production checks
remain separate work. Tests use no real user data.
