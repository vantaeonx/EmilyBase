# ADR0093: Bounded atomic service row batches

Status: accepted for experimental trusted service transport.

Add POST tables/rows/batch to both routers. A strict document selects one public
table and1..256 ordered insert/update/delete operations, using the existing exact
row/key wire. The event bound derives from MAX_TRANSACTION_EVENTS. Keep the
65,536-byte body cap before parsing; operation count and complete wire/value/
encoded-row validation follow decoding. No batch-controlled project, path, table
per operation, SQL text or transaction-lifetime handle is accepted.

Apply all prepared operations to one original staged transaction. Later operations
see prior staged writes. Every schema/key/existence/duplicate/capacity failure
drops the entire transaction before WAL commit. Successful packets append one
original durable commit and report changed-operation count plus decimal-string
transaction ID. Count measures accepted operations, including later reversed
changes, not final live rows. Empty packets refuse. No partial success or silent
fallback into individual commits is allowed.

Retain the project owner/data gate and worker permit across staging/commit even
when the client cancels. Private-root current-key recheck follows body wait before
decode. Master/user/sibling credentials grant no batch authority. Private time and
WAL are untouched. Existing rate/body/deadline, static errors and no-cache policy
apply. This does not grant public user/RLS authority or idempotent retry.

Cover mixed dependent writes, one-commit accounting, late duplicate/missing/type/
primary-change errors, exact old history,256/257 boundaries, strict fields and an
independent generated all-or-nothing model. A native child pauses inside the actual
batch staging path before commit and after response construction; kill/reopen on
WAL1/2 proves prepared operations stay absent and committed packets recover.
Real TCP checks cover both routers/formats after successful acknowledgment and
late refusal, plus duplicate-writer races. Common lifecycle grows from twelve to
thirteen successful ACK kills before backup/restore. Extend the shared decoder
fuzz target with batch input. Hardware power-loss, whole-heap admission and stable
format/production gates remain open.
