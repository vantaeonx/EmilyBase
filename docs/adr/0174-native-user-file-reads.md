# ADR0174: native user-only file observations and complete reads

Status: accepted and verified as an experimental synchronous native API.

## Decision

Add named native user-only file metadata/list/read methods without a project
service-key parameter. Keep service-key backend methods unchanged. Split the
internal original project-metadata selection from key verification; neither that
selection nor an internal owned-project capability is a user grant.

The common frame authenticates current explicit public admission and a genuine
access session before selecting the requested native file binding. Retain an actual
private WAL selection descriptor before any forward authentication clock append.
Then derive actual nativeDB and obtain the currently installed FilePolicyPrincipal
under the same original private/file/common owners. Check context even for a missing
reference or empty list. Repeat original common/project-metadata/private/native
selection checks through final output handling. No copied preflight identity grants
the operation: the installed current proof is constructed inside the actual frame.

Read authorization consumes authoritative current metadata from the actual retained
native reader. List/metadata filter visibility under the installed read rule. A
missing or denied payload returns ordinary denial without releasing bytes. A
bounded complete copy stays withheld through original reader and final common
checks. Reuse the backend copy implementation rather than relax native validation.
A genuine private borrow excludes admission/policy/credential mutation via that
store during consumption. Trusted time remains one request observation, not an
extended wall-clock lease; a forward attempted authentication can persist its clock.

## Limits

No upload, rename/remove, signed URL, HTTP/SDK file route, role system, implicit
private migration or provisioning is introduced. Physical orphans are never user
references. Existing native inventory/payload caps remain; copied values confer no
future permission. Preserve v2 manifest/private membership and all file versions.
Actual user writes need authoritative native mutation authorization and durable
publication/uncertainty tests in a separate increment.

## Acceptance

Both WALs; two projects and actual distinct users; owner/authenticated/deny rules;
closed admission, missing catalog/policy, revoked/expired/disabled credentials;
replacement without a new token; current native reference ownership; no service
key accepted as a user session; no hidden file-binding lookup on denied admission;
empty/missing handling; payload bounds and no partial output; original descriptors,
late replacement/refusal, actual signal9 kills, reopen and independent restore.
