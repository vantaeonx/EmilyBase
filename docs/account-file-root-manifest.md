# Experimental file-capable account-root roster codec

This increment defines and implements pure bounded inspection/canonical encoding
of a future file-capable root's declared roster. It does not attach a file store,
create or migrate a root, enable user uploads or extend coordinated account backup.
Ordinary AccountRoot/restore/capture continue to accept their existing v1 layout;
they deliberately refuse this new v2 manifest. The codec is functional inspection,
not a partially enabled live mode. See [ADR0149](adr/0149-explicit-file-capable-root-roster.md).

The later [native component collector](account-file-component-capture.md) produces
a copied declaration from explicit original account/file owners. It still does
not install this manifest as a live root or enable common restore/user file routes.

## Exact metadata representation

The maximum complete input is32768 bytes, checked before JSON decoding. The exact
UTF-8 JSON encoding has no whitespace/trailing bytes and this envelope/payload order:

```json
{"payload":{"version":2,"private_projects":["01010101010101010101010101010101"],"file_stores":[{"project":"01010101010101010101010101010101","database_id":"02020202020202020202020202020202"}],"reset_at":100},"checksum":2506296039}
```

The CRC32 covers exactly the canonical payload bytes, starting with `{` and ending
with `}` before the envelope checksum. It detects accidental changes only. It does
not authenticate origin, prove a live capture, grant permission or sign a backup.
Public `to_bytes` revalidates semantics and emits exact canonical bytes. Inspection
decodes bounded collections, validates semantics, then checks the decoded checksum
and exact canonical encoding. Unknown/duplicate fields, alternate escaping/key order,
trailing whitespace/data, wrong scalar types and unsupported versions refuse.

Every project/database ID is exactly32 lowercase ASCII hexadecimal characters.
IDs select future fixed directory components, never caller-supplied paths. Private
projects are strictly sorted and unique. File bindings are strictly sorted by project
and unique; each project must be present in the private roster. Every file metadata
database ID is unique among file bindings. Empty/subset rosters are explicit and
valid; no missing member is inferred or provisioned. Reset time is bounded by the
existing signed64-bit trusted-time domain; parsing/encoding never changes a clock.

Each roster is capped at128 entries, independently of the byte bound. Custom JSON
collection/identity visitors cap entry count and reject noncanonical IDs before
copying field strings. Collection reservation refusal returns an error. No database, credentials
or filesystem path is opened by the pure codec. Bounds are not a whole-process heap
reservation or a runtime allowance to open128 project/file owners.

The copied `AccountFileRootManifest`/`FileRootBinding` types expose read-only metadata
and checked constructors. A binding database ID is the native file metadata database,
not the public data/private account database. Comparing declared IDs with actual
retained owners, registry/private membership and global database uniqueness remains
required before a future attach/capture/restore can claim a consistent root.

## Explicit inspection CLI

```sh
emilybase account-file-roster-check < synthetic-roster.json
```

Redirected input is buffered under the32KiB+one detection byte bound. Only format,
private/file counts, decimal-string reset time and input byte count are emitted;
no binding IDs, name, credential or payload is printed. Stdout write/flush must
succeed. The command creates no root or files and performs no migration, admission,
clock advancement or source repair. Bad input emits no output record.

## Compatibility and future adoption gates

The old manifest reader refuses v2. The new codec refuses v1 rather than fabricating
an empty file roster. Existing v1 root initialization/open/capture/restore semantics
and file/account/bundle versions are unchanged. Merely writing a valid v2 root.json
into an existing v1 root makes current open refuse without changing other files;
restoring its original manifest allows ordinary open again. This is an executed
synthetic compatibility test, not an upgrade procedure for real data.

A later explicit offline adoption must declare fixed `files/<project>/metadata` and
`files/<project>/objects` members, retain actual root/children and original file
owners, and compare native file project/database identity with this declaration.
Missing, undeclared, duplicate or foreign members must refuse without provisioning.
The retained-root lock order, exact runtime counts/resource admission and combined
bundle version/byte/decoded-image bounds must be settled and tested. Future restore
must still reset private session scope before common selection, retain orphan charge
and permit independent subsequent writes. No user endpoint should precede executed
coordinated capture/restore for that same mode.

Migration must be explicit and preserve the source for rollback; ordinary startup
cannot silently rewrite manifests, adopt objects, reset sessions or change clocks.
Any future format change needs a new version and refusal fixtures. This experimental
codec does not make the combined root format stable or any platform stage complete.

## Executed scope

[Source-bound evidence](measurements/2026-10-15-file-root-roster/verification.json)
records a fixed golden CRC independently checked with a separate implementation,
empty/subset/128-member round trips, canonical IDs/order/membership/database uniqueness,
16 checksum-repaired semantic negatives, duplicate/unknown/alternate JSON forms,
every truncation/single-byte high-bit mutation, oversized inputs and typed refusal.
Two128-case generated models cover valid sorted subsets and arbitrary bounded bytes.
Actual legacy/current root refusal preserves every other source file and creates no
file-store directory. Three actual CLI cases cover counts, refusal, no filesystem
creation/source modification and failed delivery. The dedicated sanitizer fuzz
target checks arbitrary inputs plus repaired canonical order/CRC to reach semantics.
These are metadata integrity/compatibility results, not user/file authority evidence.
