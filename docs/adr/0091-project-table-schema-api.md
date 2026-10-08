# ADR0091: Project table schema operations over service HTTP

Status: accepted for experimental trusted service transport.

Expose GET tables and POST tables/schema, tables/create and tables/drop under
both authorized project routers. Use the original synchronous catalog, snapshot
and transaction engine directly. Creation validates and physically encodes the
schema before opening a transaction. Drop removes the table and its rows in one
original WAL commit. Existing names refuse; recreation receives a new stable ID.
Neither operation changes the stored format or introduces a database dependency.

Return inventory in ascending live table-ID order, at most128 entries, containing
name, ID, column count and primary-column name. Return a complete schema on a
strict table-name request. IDs and acknowledged transaction IDs are decimal JSON
strings, preserving u64 precision in clients. Create/drop success is HTTP200 after
commit. Enforce the existing65,536-byte input/output boundary without truncation;
metadata is additionally bounded by128 tables,64 columns and63-byte identifiers.

Retain project directory owners and the data gate across synchronous operations.
Keep four admitted workers, socket rate limits, five-second body deadline and
blocking parsing/recovery/serialization. Private-root mode checks current service
credentials after body waits and before parsing; legacy admitted-capability policy
is unchanged. No private clock observation or private-WAL write is required.
Success and refusal disable caches; errors and route logs contain no input data.
Master credentials and user sessions are not project-service authority.

Validate exact history, strict fields, maximum inventory, generated create/drop
sequences, sibling boundaries and key replacement during body waits. Native TCP
checks kill after create/drop acknowledgments on both WAL versions and routers,
then verify fresh IDs and absence of dropped rows. Extend the shared native and
container lifecycle from seven to nine acknowledged kills. Whole-process heap,
user authorization/RLS, stable format and production gates remain open.
