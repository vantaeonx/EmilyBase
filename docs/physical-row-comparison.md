# Physical row verification without copied payload

Snapshot row-location resolution still checks current table/key/address, selected
slot, record digest, complete ETBL/EROW format and live row equality. The validating
codec now borrows text/bytes when comparing with the immutable model row, instead
of materializing another owned row and long primary key. The same cell parser
serves owned decode, preserving its format checks and stored float bits.

A mismatched early field never skips subsequent validation. Invalid versions,
tags, Boolean/float/UTF-8, sizes and trailing bytes remain errors. Signed zeros
retain the original numerical equality. Valid physical divergence remains a stale
location. Borrowed cells cannot escape their source; no unsafe code is introduced.

A warmed source-bound sample resolves1000 rows. Requested allocations decrease
from3344000 bytes for integer keys/wide text and6416000 for long text keys to104000
in either case, peak104 and zero retained. Per-call schema validation still
allocates; metadata/cold models/caches are separate costs. These are summed
requests, not concurrent memory, RSS or throughput. See [observations](measurements/2026-10-07-physical-row-match/operation-allocations.json)
and [ADR0063](adr/0063-compare-physical-rows-with-borrowed-codec.md).

Stored formats, versions, SQL/API, durable acknowledgement and async boundaries
are unchanged. Numeric model/cache/staging/transient admission, combined durable
writer and production acceptance remain open.
