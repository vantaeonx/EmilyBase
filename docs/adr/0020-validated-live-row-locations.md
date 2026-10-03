# ADR 0020: validated live row-image locations

Status: accepted for the bounded synchronous snapshot API.

## Context

The standalone B+ tree already stores page/slot pointers, but relational history
contains obsolete insert/replace images. Reading a historical slot alone can
return deleted or updated data. Staged transactions also reuse the same future
page/slot after rollback. Locations need validated liveness before an index can
use them. Durable table indexes remain a separate increment.

## Decision

Track each live table/key's latest insert/replace position while staging and
replaying existing ETBL events. The location holds table ID, sequential page ID,
slot ID and SHA-256 of the full encoded event. Compute fallible key extraction
before publishing state, pages or the location map. Update/delete/drop retire
previous positions. Failed writes and discarded staged snapshots publish none.

Resolution first requires exact equality with the current table/key location,
then checks the slotted record, event fingerprint, table ID, primary key and row
image against current validated state. Return a typed stale-location error on
substitution. Debug output redacts fingerprints; parsing errors never echo JSON.

The lower-level Snapshot selects its own scope and has no persistent database ID.
Managed Database and Transaction wrap locations with the existing persistent
16-byte WAL identity. Independent databases reject each other's bound locations,
including byte-identical rows and physical positions. CLI row-location/row-resolve
operate on managed databases, with the ordinary bounded strict JSON parser.

## Compatibility and limitations

No storage, event, WAL or backup bytes change. The map is rebuilt from committed
history and is not another durable source of truth. Both WAL versions, baseline
compaction, checkpoint loss and verified restore preserve positions because they
retain physical relational page images. A future history vacuum or format upgrade
may invalidate locations and must document that behavior before implementation.

A location identifies a current row image, not an unforgeable capability,
transaction ID, durable change stream offset or public API access grant. It needs
ordinary data authorization. Identical bytes at an identical position in two
staged branches are the same image; SHA-256 does not encode transaction lifetime.
Restored copies intentionally retain database identity. Either copy rejects an
address once its corresponding row image changes, independently of the other.

The map adds bounded live-key/location memory and transaction clones alongside
existing primary-key/row maps. Those maps still use BTreeMap. The standalone index
still limits text keys to 256 bytes while tables permit 3072; this increment does
not narrow table keys, persist index roots or claim atomic table/index WAL replay.
The index adapter, allocation/retirement and durable index transaction design
remain open. Fingerprints are integrity comparisons, not secret authenticators.
