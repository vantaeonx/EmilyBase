# ADR0089: Bounded synchronous logical table transfer

Status: accepted for experimental synthetic offline exchange.

Add emilybase-transfer as an independent workspace crate using the original
catalog/snapshot/transaction APIs. Keep serialization and validation synchronous;
no async engine or existing database runtime is added. CLI export writes the whole
bounded document to stdout; CLI import reads stdin before opening the target.
No new filesystem publisher is introduced.

Use a versioned strict JSON envelope with explicit schema and typed primary-ordered
rows. Borrow export rows through the checked physical primary cursor. Preserve
finite binary64 exactly using canonical16-character lowercase float_bits text.
Do not silently normalize nullability, keys, names, types, order or duplicates.
Bound bytes, arrays and text before retaining excess values; validate original
encoded schema/row limits before accepting an immutable verified input.

Set the row limit to255, reserving one of the engine's256 transaction events for
CREATE. Import creates a new table in one durable transaction; existing tables
refuse. It neither appends nor merges nor creates a database. Large table transfer
requires a separately designed continuation/atomicity protocol. This cap is a
visible limit, not a completed unlimited importer.

This editable logical format carries no backup integrity/authenticity claim, WAL
history, project identity, verifier/session lifecycle or credentials. Prefer common
account-root bundles for private restore. Stored file/WAL formats stay unchanged.
CLI output is explicit plaintext; the caller owns file permissions, fsync and
publication. A committed import can lack an observed response; inspect before
retrying. Typed errors do not embed record contents. An independent PostgreSQL or
Supabase converter, public authorization and production gates remain open.

Tests use independent synthetic models, full row/event boundaries, exact finite
bits, malformed/oversized/nested documents, source/existing-target WAL preservation
and actual CLI pipes/reopen. Native helpers pause the actual import immediately
before commit or after acknowledged return, then are killed and reopened on WAL1/2.
The new parser has a dedicated sanitizer fuzz target.
