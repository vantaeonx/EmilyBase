# Current-policy filtered native pages

The synchronous owned root gateway accepts `UserTableOperation::Page { after,
limit }` and returns `UserTableResult::Page { rows, next }`. It requires the same
current project service key, current access token and trusted time as
[owned CRUD](user-row-enforcement.md). A service key belongs only in trusted code.
The [trusted backend HTTP adapter](user-row-http.md) uses both credentials.
There is no user-only/browser credential contract or SQL permission.

Limits are 1..128 rows, checked before private verification/clock observation.
The root derives the actual table identity and complete schema and holds both
original owners through verification and reading. Context is checked even for an
empty table or an after-end request. Every returned row passes the current SELECT
rule; denial skips that row. Other policy, key, storage or physical-index errors
fail the complete page without returning an earlier partial prefix.

Rows are ordered by original primary key: signed i64 numerically or text by UTF-8
bytes. After is an exclusive typed key; it need not still exist or have been
returned by a previous page. Wrong key type or a value beyond original bounds
refuses, including on empty tables. It is a seek value, never an authorization
capability. Supplying a different key cannot bypass the current policy.

A continuation is the last returned permitted key only when a further permitted
row exists. Hidden gaps and trailing rows yield no key, hidden count, scan watermark
or phantom next-page indication. One visible lookahead is authorized without
cloning its result payload. At most 128 selected rows are retained; unfiltered
rows are traversed through the original borrowed, physically checked primary
cursor. The public database has an existing 10,000-live-row bound, so a page can
inspect up to that bound when many rows are hidden. Existing policy compilation
limits each rule as documented; no unbounded user expression is executed.

Each selected row obeys the existing 4,000-byte encoded-record cap. This bounds
retained physical result payload at 512,000 bytes, not Rust allocator overhead,
scratch, derived indexes or total process memory. A future HTTP adapter must
independently admit and bound transport/worker output. The current backend adapter
caps complete JSON responses at 65,536 bytes and can therefore refuse a native page
that fits the native row count; request a smaller page explicitly.

Pages observe current state on each call. Deleting a cursor key does not break
continuation. Inserts before an already used cursor are not revisited; later
inserts, owner changes and policy replacements may alter the remaining results.
Sessions/revocation/table/schema are rechecked on every call. There is no stable
multi-request snapshot, signed cursor, frozen policy or automatic retry. Public
history never changes from a page. Equal trusted time also leaves private history
unchanged; a forward private time observation retains its separate commit rules.

Checks cover hidden tails, maximum visible page/lookahead, empty results, deleted
and arbitrary cursors, current policy/session/recreated tables, wrong schema even
when empty, extreme signed integers, 3,072-byte Unicode/NUL keys, independently
generated owner maps, all-hidden full-capacity sources, large physical result
payloads and verified common-root restore with fresh sessions. See
[verification](measurements/2026-10-09-current-policy-user-pages/verification.json)
and [ADR0105](adr/0105-current-policy-filtered-keyset-pages.md).

Custom ordering, roles, public user-only HTTP, timing/other side-channel protection,
complete resource/load/upgrade/security and production gates remain open. Use
synthetic data only.
