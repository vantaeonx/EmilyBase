# First private project without an input archive

The offline Rust CLI creates one empty project plus its session-ready private
store. It needs no database engine, prepared bundle, password or signing key.
Use disposable synthetic data only; this remains experimental.

```sh
cargo run --locked -p emilybase-cli -- account-root-init account-root \
  --name synthetic-project --reset-at 0
cargo run --locked -p emilybase-cli -- account-root-verify account-root
```

The required --reset-at value is the trusted initial Unix-time floor, an unsigned
decimal no larger than signed i64 maximum. Zero is suitable for this disposable
empty example; no prior user/session authority exists. Names allow128 UTF-8 bytes
and no control characters. Invalid values fail before filesystem work. Select a
new target under an existing trusted parent. Existing targets are never replaced.
The documented account-root directory and hidden initialization stages are ignored
by Git. Keep operational data outside a source checkout.

Output contains counts and the time floor only: one project, one private store,
zero public tables/rows/users/session families. No plaintext project key is printed
or retained in a receipt file. The initially generated key is discarded; a trusted
operator obtains a usable key through explicit rotation after starting the server.

Supply a private EMILYBASE_MASTER_KEY using the existing operator configuration,
then start the explicit native mode:

```sh
env -u EMILYBASE_DATA_DIR EMILYBASE_ACCOUNT_ROOT=account-root \
  cargo run --locked -p emilybase-server
```

Use the authenticated GET /v1/projects operator route to obtain the generated ID,
then POST /v1/projects/{id}/keys/rotate with the master credential. Keep the returned
service key on a trusted backend. With that key, POST the bounded JSON login/password
to /v1/projects/{id}/auth/users, then /auth/sign-in. See the exact
[private transport](private-http.md) and [OpenAPI](openapi.json) contracts.
User tokens still grant no SQL access; public signup, roles and row policies remain open.

Creation compares complete original-engine histories and retained owners before
atomic selection, and flushes the publication parent before success. A reported
unknown outcome can already have a complete selected root: verify it instead of
blindly retrying. Failed preparations remain as private .emilybase-account-init-*
directories for trusted inspection; do not adopt or recursively delete them blindly.
No stage is discovered or reset during server startup. Normal restart preserves
sessions; [verified root restore](account-root-restore.md) changes clone scope.

[ADR0083](adr/0083-explicit-private-root-initialization.md) records the protocol and
late-substitution regression. Whole-process memory/staging admission, container
root-mode deployment, dynamic private roster and production acceptance remain open.
