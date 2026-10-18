# Explicit native v2 account/file root admission

AccountRoot::open_v2(path,pool) and open_v2_at(parent,leaf,pool) explicitly admit an
existing canonical v2 root. Default open/open_at, offline v1 inspection/capture and
ordinary server-binary startup retain their v1-only contract. No entry is created,
format migrated, session scope reset, clock advanced or public admission enabled
by normal opening. Real data and production acceptance remain outside this work.

```sh
emilybase account-file-root-info ./copies/restored
```

This actual CLI opens all declared original owners and emits only root_format,
projects, private_projects, file_stores and decimal-string reset_at. It prints no
project IDs, names, rows, payloads, password verifiers or tokens. Failed stdout exits
unsuccessfully and releases owners without modifying valid source histories.

## Layout, ownership and admission

A private RootLayout enum retains the full original v1 or v2 manifest. The v2 path
requires exactly root.json, registry, private and files. Parse the original private
manifest through its retained no-follow/nonblocking file, under the existing32768
byte canonical codec bound. Admit at most four private stores and four file stores
before opening registry/private/file models. V1 retains its existing8192 byte bound.
The codec's broader roster domain does not increase active service admission.

Require exact private/file rosters. Open only explicitly bound native project leaves
relative to the original files directory. Validate every persisted file scope/model,
quota and physical object inventory including orphans, and match its metadata database
ID to the declared binding. Refuse ID reuse across public, private and file databases
while actual owners remain held during registry/private validation. Missing sources,
wrong binding IDs, extra members, unsafe modes/links or observed corruption refuse;
opening does not provision missing components or repair complete corrupt histories.

Keep the actual NativeFileRoot owners and their original parent alongside account
owners. Drop component owners before directory anchors and the common root lock.
Readiness verifies the original manifest, exact rosters, containing directories and
native file root/scope/WAL selection. A same-byte replacement of any selected root,
metadata directory, WAL, scope marker or manifest cannot become a new owner.
The immutable native check_selection API exports no descriptor and does not rehash
all payloads on unrelated account operations; initial/full native operations perform
physical inventory validation. These finite checks provide no namespace lease.

The common root, registry/private and native file owners remain retained while the
root is active. Public database writers span initialization capture or individual
data operations and then unwind, matching v1 behavior. This does not promise that
all public writers or individual payload readers remain locked for the whole root
lifetime. File binding metadata and persisted owner fields grant no current user
file permission. No mutable file handle escapes this account API.

Native open_v2_at retains its original parent after caller release/movement and
rejects a foreign former pathname. Path-based open_v2 keeps its original path
selection contract. Wrong supplied parent comparisons do not replace source owners.

## Usable operations and deliberate restrictions

Existing account administration, sign-in/refresh, access verification and trusted
project SQL operate on an explicitly admitted v2 root. Normal reopen preserves the
newly issued sessions and acknowledged public writes. Old source sessions remain
invalid in a separately restored copy; opening alone does not reset another scope.
The full v2 manifest/file binding bytes remain unchanged by these account operations.

A native Rust caller may explicitly construct account_router with that owned root.
Actual TCP tests verify project-key SQL admission, master-key refusal for project SQL,
acknowledged mutation and graceful restart under both WALs. The ordinary server
binary has no implicit v2 selection: explicit configuration/admission and a binary
recovery/container drill remain separate work. User-authorized file routes are absent.

V1 capture and backup refuse v2 before staging, including the joined backup helper
when given a v2 root. Returning only its account subset would silently lose file
bindings. Complete rebackup of internally owned v2 components needs a separate
checked coordinator with real split borrows, preserving original file owners through
publication. Do not bypass the refusal, close/reopen components or reinterpret the
v2 manifest as v1 authority. This limitation must close before ordinary v2 deployment.

## Executed coverage and open gates

[ADR0164](adr/0164-explicit-v2-account-root-admission.md) records the decision.
[Source-bound verification](measurements/2026-10-19-explicit-v2-open/verification.json)
records final commands/hashes and exclusions. Ten new server cases cover empty/one/
four-project roots, both WALs, account/public mutations, session-preserving restart,
real TCP integration, legacy refusal, exact file bindings, cross-component aliasing,
early excessive-roster admission, privacy/types/corruption without repair, six active
same-byte replacements, nine opening-time changes, retained parent movement and16
generated models. Eight actual signal9 kills observe component-opened, inventory-
validated, service-opened and actual returned-owner boundaries on empty WAL1 and
one-project native WAL2 inputs, checking unchanged histories and released owners.

Five actual binary cases verify private count output, legacy/v2 separation, wrong
bindings, busy owners, parent aliases/modes, corruption, lost stdout, exact maximum
time and native non-UTF8 root names. Full affected server library tests run on both
toolchains, alongside related auth/registry/CLI/core/native checks. No new parser,
persisted version or dependency is introduced; no fresh sanitizer/production audit
or deployment is claimed. Rebackup, binary configuration, aggregate admission,
roles, user file policy, realtime, dashboard and release acceptance remain open.
