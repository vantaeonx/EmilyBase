# Explicit native object write limits

`WriteLimits::new` validates explicit per-call limits within0..128 objects and
0..64 MiB total payload. `ProjectDirectory::put_bounded` enforces them against a
fresh complete verified inventory under the retained cooperating owner lock.
Per-object input remains0..8 MiB. Empty objects charge one name and zero payload
bytes. Object IDs remain immutable and cannot be overwritten.

These limits are chosen by the trusted native caller for this operation. They
are not persisted configuration, an authorization policy, a service upload quota,
rate limiting or a global heap reservation. The existing native `put` remains a
separate filesystem operator primitive; it can create inventories exceeding the
bounded capture/write limits. Future services must bind limits to authoritative
configuration and route every writer through the admitted operation. Do not
expose the native primitive as a user-authorized upload API.

The operation:

1. Refuses oversized object input before scanning or encoding it.
2. Verifies the complete current directory, then checks duplicate identity, count
   and aggregate bytes. Unknown/corrupt/foreign entries refuse admission.
3. Computes the new object hash and fallibly allocates at most128 metadata entries
   for the complete expected sorted inventory/digest, before creating a stage.
4. Rechecks the entire current inventory, then uses the original immutable put
   with exact file readback, no-replace selection and file/parent fsync.
5. Rechecks the complete final inventory against the expected receipt. A mismatch
   after selection is `PublicationUnknown` and never triggers a delete/retry.

`WriteLimits::check` is a pure metadata check only. Its result does not reserve
capacity or grant user access. Returned `WriteReceipt` contains object/hash/size
and the complete checked metadata snapshot; it contains no payload and grants no
lease against later native filesystem changes. Cooperating owners serialize
writes. Same-UID administrators and operator paths remain trusted.

Capacity is recomputed from current checked files on each operation/reopen. No
volatile counter is trusted across a restart. Lowering per-call limits does not
evict existing data; it refuses an operation that would remain over capacity.
An unreceived selected write can occupy capacity and must be explicitly inspected.
There is no automatic retry, overwrite, orphan sweep or policy reinterpretation.

```sh
emilybase object-directory ./objects 01010101010101010101010101010101 put-bounded 02020202020202020202020202020202 --max-objects 128 --max-bytes 67108864 < synthetic.bin
```

The CLI validates IDs/limits before reading stdin. Zero allowed names refuses
immediately. Redirected input is bounded by the smaller of8 MiB and the supplied
aggregate byte limit, plus one detection byte; it stops on overflow even if the
writer keeps the stream open. It buffers before acquiring the directory lock.
The complete remaining capacity is then admitted against current files.

Success prints bounded metadata only: format/project/object/object bytes/hash,
complete object count/total bytes/digest. A stdout failure preserves the selected
object; inspect the directory before deciding what to do next. Restored copies
are admitted by their current inventory, not inherited volatile counters.

Multiple full inventory read passes bound logical work but do not provide a
service latency/RSS budget. Persisted quota policy, HTTP uploads, coordinated
AccountRoot membership, file permissions/signed URLs and production acceptance
remain open. Process-kill tests do not qualify power-loss behavior.
