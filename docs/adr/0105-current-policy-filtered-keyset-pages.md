# ADR0105: Current-policy filtered native keyset pages

Status: accepted for native current-policy pages after the checks below; no user HTTP route.

Extend the synchronous owned user-row gateway with Page { after, limit }, returning
only SELECT-permitted rows in ascending primary-key order. Limits are 1..128 and
checked before private verification/clock observation. The existing real-table
context check runs even for empty tables, no matches and after-end requests.
Original borrowed primary_rows validates key types and physical row/index pointers;
the original public database is already capped at 10,000 live rows. No unfiltered owned scan is made.

After is an exclusive typed key, not an authorization capability. Every request
rechecks current service key/session/policy/table/schema under both original owners.
Only visible rows are retained and a continuation contains the last returned key
only when another permitted row exists. Hidden trailing rows yield no continuation;
no hidden key, count, scan watermark or unfiltered row is returned. One visible
lookahead is checked without cloning its result payload. All errors discard the
partial page and return no rows. At most 128 existing 4,000-byte encoded rows are
copied; this count/physical payload bound is not total heap or HTTP admission.

Pages observe current state separately; they are not a multi-request snapshot.
Deleted cursor keys remain usable; inserts and policy/owner changes are reflected
on each new call and can alter remaining results. A caller-supplied key may seek
anywhere but never bypasses current SELECT. No writes, automatic retries, SQL,
roles, custom ordering, public HTTP or signed cursor are added. Forward private
clock observations retain ADR0104's separate commit semantics. Timing and other
side channels, worker/resource/HTTP admission and production gates remain open.

Acceptance requires hidden gaps/tails and lookahead, exact maximum, deleted and
arbitrary cursors, large signed integers and long Unicode/NUL text keys, invalid
limits/types/bounds and empty mismatched context, current policy/session/recreated
identity, independent owner-filtered models, reopen/verified restore and exact
public/private history at equal time. Stable/minimum tests and strict compile/lint
checks must pass before acceptance.

Frozen-source checks pass all 17 owned-user-row cases on Rust stable/minimum:
eight new page cases and nine existing CRUD/current-proof/recovery cases. Each
toolchain exercises 16 independently generated page owner maps plus the existing
24 mutation sequences and four staged/received-result packet kills on WAL1/2.
The full 10,000-hidden-row source and 128 large-payload results are checked.
Strict formatting/workspace/fuzz Clippy, minimum workspace build and all-target
fuzz compilation pass. No request parser, format or HTTP route changes; no new
sanitizer campaign is claimed. See
[source-bound evidence](../measurements/2026-10-09-current-policy-user-pages/verification.json).
