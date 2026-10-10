# Standalone native file catalog

The `emilybase-files` crate combines a separate original managed Database and one
native ProjectDirectory. Metadata uses EmilyBase's own tables, fixed pages and
mandatory WAL; no external database engine is introduced. This is a synchronous
operator library, currently outside server/CLI dependencies and AccountRoot.

Trusted native callers acquire the metadata Database first, then the object owner,
and pass both by ownership to `FileStore::initialize` or `FileStore::open`. Neither
owner or arbitrary SQL access is exported. Initialize requires the original
single-marker pristine database and an empty already-initialized object store;
it commits both private schemas and one scope/quota row in a single transaction.
Existing paths, dropped-table history, orphans and partial initialization are never
silently adopted. A late initialization failure is an uncertain result requiring
inspection. Open validates recovered metadata and the complete object graph.

## Private logical schema version1

There are exactly two tables with the documented schemas and no nullable columns.

| Table | Fields |
| --- | --- |
| file_scope | integer primary ID1, integer version1, project16 bytes, metadata database ID16 bytes, physical object limit, physical payload-byte limit |
| file_references | canonical lowercase32-hex logical primary ID, immutable object ID16 bytes, owner metadata16 bytes, bounded UTF-8 display name, payload length, SHA25632 bytes, commit revision8 bytes little endian |

Exactly one scope row binds the supplied project and metadata database identity.
The maximum is128 references and129 total rows. Each reference binds one distinct
physical object, exact length/hash and a positive revision no greater than the
metadata WAL's current transaction. FileId is distinct from ObjectId. FileInfo is
opaque expected metadata; its Debug omits owner/name/payload. The pure
`inspect_file_record` validates row shape without filesystem or account authority.

Display names have1..256 UTF-8 bytes and no control characters. They are never
interpreted as paths or HTTP headers; even `../../synthetic.bin` remains display
text. Paths use fixed typed IDs. Owner bytes come from the trusted native caller;
they do not prove account existence, a current session, admission or file policy.
The future account/registry mapping is deliberately outside this native contract.

The persisted operator FileQuota bounds physical objects0..128 and payload
bytes0..64 MiB, with8 MiB per object. Publish accepts no per-call quota override.
Headers, metadata history, operating-system caches and total process memory are
outside this payload quota. A zero object limit is a valid closed store. Existing
valid unreferenced objects count exactly like referenced objects. Corrupt/unknown
inventory, missing references, duplicate object references, scope/hash mismatch or
already-exceeded quota refuse open and operations without repair or deletion.

## Publication and visibility

1. Validate bounded input, the complete current private catalog, complete native
   inventory, persisted quota and fresh logical identity under both owners.
2. Publish the fresh immutable object through the existing bounded private
   no-replace/file-sync/parent-sync path. Retain SelectedWrite's actual descriptor,
   original owner and expected complete inventory receipt.
3. Revalidate that complete receipt and selected inode. Insert the exact reference
   with the next transaction revision and commit through the original WAL.
4. Revalidate committed schema/scope/quota/reference graph and the complete native
   receipt, still retaining the actual selected descriptor, before returning success.

SelectedWrite's additive `verify_complete` scans complete inventory twice with
actual selected-object verification before, between and after. `verify` retains its
earlier exact-object-only meaning. Neither method refreshes expected metadata or
leases later namespace state. A sibling change after the last inventory observation
can be detected by a subsequent check; no arbitrary hostile filesystem atomicity
is inferred from these observations. Cooperating native owners remain required.

Every error after blob selection maps to an uncertain file result and poisons the
FileStore. Original native publication uncertainty also prevents reuse. Drop/reopen
and explicit inspection resolve current durable state; the library never retries,
overwrites, cleans up an orphan or invents a rollback of a completed file sync.
Failures before metadata commit can leave a valid private unreferenced blob.
It remains absent from list/info/reader but charged against physical quota.
A lost response after WAL commit can leave a valid reference without caller success;
this is a committed transaction, not an uncommitted transaction appearing on restart.

`list`, `info`, `usage`, `quota` and `reader` validate the full current graph under
both owners. Only committed logical IDs select payload readers. The returned native
ObjectReader borrows FileStore, retaining the metadata/object owner lifetimes and
checking expected length/hash. There is no raw-object-ID download escape in this API.
Earlier native operator tools retain their separate filesystem authority.

## Compatibility and limitations

This creates a separate private logical schema; existing database/WAL/object/archive
byte formats and fsync acknowledgments are unchanged. Only this schema's version1
is admitted. Future changes need an explicit migration and evidence. Reconstructed
matching object directories may have fresh inodes after a legitimate restore;
there is no persistent unique object-directory UUID in the older native marker.
Project/complete graph matching must not be described as proof of unique ancestry.

Native [metadata rename/logical deletion](native-file-mutations.md) now use exact
reference CAS and retain the source through the own-WAL commit. Deleted references
leave charged physical blobs; quota administration, idempotent request identity,
orphan reclamation and catalog growth/upgrade policy remain follow-up work. This
library does not add a current account policy, user HTTP, signed URL, Root manifest,
coordinated backup/restore, dashboard, network admission or production acceptance.
Independent metadata/object archives must not be advertised as one coordinated copy.
See [ADR0135](adr/0135-original-wal-native-file-reference-catalog.md) and the open
[AccountRoot proposal](adr/0127-proposed-account-root-object-integration.md).

## Executed evidence

See [source-bound verification](measurements/2026-10-10-file-catalog/verification.json).
Checks cover independent reopen, persisted quotas, orphan invisibility/accounting,
canonical IDs, display-only traversal strings, exact maximum sizes/counts, generated
reference models, semantic metadata/graph corruption and identical-byte selected
inode replacement around the actual WAL commit. The process-kill matrix covers
blob-only, committed-reference/no-response and acknowledged-reference boundaries
with empty and8193-byte binary payloads, followed by independent writes after recovery.

Stable1.99 and minimum1.89 each passed559 affected checks:238 native files/storage/
CLI and321 database/WAL/transactions/backup. Eleven FileStore cases, one additional
complete-receipt case and one FileStore-owner compile-fail case are new. Generated
reference histories run24 models with0..7 payloads and reopen after every publish.
Fifteen semantic metadata mutations refuse without repair. Actual128 empty
references and an8 MiB object fit the original engine and exact persisted capacity.
Six new native process kills per toolchain complement25 existing native boundaries.
These new reference/kill cases use default WAL1; existing affected core cases also
cover WAL2, without claiming new catalog-specific WAL2 acceptance.

All592 frozen source/configuration hashes match. Formatting, strict workspace/fuzz
lint, builds and minimum all-fuzz compilation exit0. The new file_reference ASAN
campaign executes10,853,168 inputs in46 seconds from72 synthetic seeds, with an
independent row admission model and canonical metadata comparison; no findings,
4096-byte input bound,446 MiB reported maximum RSS under a512 MiB campaign limit.
That finite campaign is not filesystem sanitizer or independent security-audit proof.
The refreshed known-advisory checks report no findings/warnings for167 workspace
and138 fuzz packages; no external dependency version was added or changed.

The first runtime check caught a real initialization mistake before publication:
the original database marker counts as event one. The fix requires exactly that
marker, zero tables/rows and next table ID1; dropped-table history still refuses.
The failing primary case and eight mutex-poison followers are recorded separately
from the final passing matrices. No production or whole-current-workspace result
is inferred from this affected-source evidence.
