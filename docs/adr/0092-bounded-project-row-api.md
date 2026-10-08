# ADR0092: Bounded typed project row transport

Status: accepted for experimental trusted service HTTP.

Add POST tables/rows/get, page, insert, update and delete to both project routers.
Use the same retained ownership, four-worker permit,65,536-byte body, five-second
body deadline and socket rate gate as table schemas. Private-root mode rechecks
current service authority after body waits and before parsing. Master credentials
and user sessions grant no row authority. No private time observation or private
history write occurs. Keep success/refusal uncached and errors/log labels static.

The original catalog validates every row and key; original transactions commit
one mutation per request. Update replaces a whole existing row and preserves its
primary key. Duplicate insert and absent update/delete refuse without commit.
Get returns an explicit null row if absent. Use the checked physical primary cursor
for reads, including long text keys, and propagate validation errors.

Expose a distinct v1 typed row wire: canonical signed decimal strings for i64,
16 lowercase hexadecimal bit digits for finite binary64, tagged null/boolean/text/
byte-array values. Preserve negative zero and avoid browser integer precision loss.
This differs deliberately from the logical table document's numeric integer wire.
The pure public grammar validator shares the exact runtime decoder for fuzzing;
it grants no authority and cannot check a selected database schema.

Page limit1..128 is explicit. Seek the supplied inclusive core bound, skip an exact
continuation key, and return a strictly exclusive page plus the last returned key
only when a checked lookahead finds more. Deleted/nonexistent continuation keys
still seek correctly. Every request sees current state; no stable cross-request
snapshot or signed cursor is promised. Borrow row bodies during serialization.
A byte-capped writer refuses the entire response above65,536 bytes before retaining
excess serialized output. Callers can reduce page size. Input JSON is body-bounded;
array/column/value/encoded-row validation follows parsing. No whole-heap claim.

Cover exact extrema and generated float/CRUD models, strict malformed input,
byte-cap refusal, long keys, concurrent duplicate insert, private/sibling history,
user-token refusal and waiting-body key replacement. Real TCP ACK kills execute
insert/update/delete on both routers and WAL versions. Extend the common native/
container lifecycle from nine to twelve kills. Include a bounded ASan parser run.
No idempotency, batch transaction, public role/RLS, stable-format or production
gate is completed by this transport adapter.
