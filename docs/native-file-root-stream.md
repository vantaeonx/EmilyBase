# Retained native common-root payload streaming

`NativeFileRoot::reader(logical_id)` admits an existing committed reference after
original acknowledged WAL/full graph verification. It retains the actual original
ObjectReader and borrows the common selection, database, object owner and shared
poison state. The borrow prevents releasing the root or mutating metadata while
the cursor lives. Missing logical references, including physical orphan IDs, are
Missing and do not become downloads. This remains a native operator primitive;
it grants no account/session/file permission.

The cursor stores bounded expected FileInfo and position, without a full payload
image. Admission and final verification still perform bounded original WAL/graph
work; this is not a zero-allocation or global-resource-reservation claim.

## Read, seek and final verification

`read_payload` copies at most8192 payload bytes, excluding the object envelope.
Before and after the original bounded object read, it checks common parent/root/
children/scope/WAL selection, original database ownership, exact committed reference
metadata and actual selected object. The original ObjectReader's finite
`check_selection` checks scope, private metadata/stamp and visible inode without
rehashing the payload. It poisons that reader on failure. A copied equal object
cannot replace the selected inode. Complete hashing remains admission/verify work,
rather than an entire-object hash for every8KiB chunk.

EOF and empty buffers still check source. Any failed read clears only the attempted
prefix, preserves bytes after that prefix and leaves the wrapper's reported cursor
unchanged. The root and reader become poisoned; reads, seeks and verify then refuse.
Underlying cursor progress from a failed outer check is never reused. Earlier
successfully returned chunks cannot be revoked.

`seek_payload` is relative to payload start/current/end, permits positions beyond
EOF and rejects negative/overflowing arithmetic without poisoning a healthy cursor.
Valid seeks check original selection around the original seek; a source failure
poisons and preserves the wrapper's prior position. `verify` hashes the complete
retained object and verifies the whole current physical catalog/quota graph, then
rechecks original selection. It preserves position. Full inventory is not rehashed
on each chunk; corrupt unrelated orphans are detected by final graph verification.

`finish` consumes the cursor after full verification and returns copied FileInfo.
It does not assert EOF or successful response delivery. Dropping early releases the
borrow without silently claiming a completed verification. Successful reads do not
modify WAL, references, physical quota or object bytes. Native finite checks cannot
isolate arbitrary concurrent same-user filesystem mutations.

## Native binary CLI

```sh
emilybase file-root-cat root 01010101010101010101010101010101 02020202020202020202020202020202 > synthetic-copy.bin
```

The command parses canonical identities and opens the original native root. It
reads/writes at most8KiB per iteration, performs final complete verification and
flushes stdout. Stdout contains only raw payload bytes; it contains no JSON, native
header, name or owner metadata. Error messages contain no payload/display text.
It never creates a new reference, repairs a source or adopts an orphan.

Binary output can already be partially delivered when a later source/write/check
fails. Exit failure does not retract those bytes; callers must require success
before using a downloaded output. Broken/failed stdout releases owners and preserves
source. No network timeout, response atomicity or user-facing download route is added.

## Evidence and remaining work

[Source-bound verification](measurements/2026-10-15-native-file-root-stream/verification.json)
covers WAL1/2, empty/1/8192/8193/maximum8MiB payloads, readonly objects, exact cursor,
invalid seeks, chunk bounds, original object descriptor, both owner locks and exact
descriptor return. Two compile-fail tests enforce root/mutation borrow lifetimes.
Sixty-four generated slice/seek histories match an independent payload model.

Seventy-two same-byte root/child/WAL/scope/mode/roster/object substitutions and
same-inode object rewrites span admission, copied bytes, seek and verification on
both WAL versions. Eight parent replacements refuse; empty/EOF/pre-read changes
clear only the attempted prefix. Corrupt unrelated orphans fail final graph checks
without repair. Sixteen actual SIGKILL cases span admission, copied, verified and
finished streams on both WAL versions with empty/8193-byte payloads. Reopen preserves
exact history/payload/charge and permits a further independent mutation.

Actual CLI tests cover exact empty/binary/maximum output, relative paths, missing/
orphan/malformed/foreign/busy/corrupt refusal, failed stdout and a4096-byte pipe.
Three blocked-output substitutions retain both owners, preserve exactly the prior
8192-byte delivery and refuse the next chunk after root/object/WAL replacement.
The final source and already changed bytes remain intact.

File/schema/dependency versions and original fsync/ACK rules are unchanged.
AccountRoot inclusion, current file policy, user HTTP/signed URLs, service-wide
admission, physical power loss and production gates remain open. See
[ADR0148](adr/0148-borrowed-common-root-payload-cursors.md).
