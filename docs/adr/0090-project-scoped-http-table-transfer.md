# ADR0090: Project-scoped bounded HTTP table exchange

Status: accepted for experimental trusted service transport.

Expose the existing logical table format through POST tables/export and
POST tables/import beneath the authorized project path in both data modes.
Export takes a strict table-name JSON object. Import takes the logical document
itself, not SQL or a caller-supplied filesystem/project path. Keep the original
four-worker admission, socket rate limits and five-second body deadline.
Use the existing65536-byte request limit and the same export response cap. A
bounded serializer enforces the stricter cap before retaining excess output;
never return a truncated table. The standalone CLI limit remains8 MiB.

Use the original AuthorizedProject data gate, retained directory owners and
synchronous engine. Private-root mode also checks the current key after the body
wait and before decode/operation. Legacy mode preserves its already admitted
request capability semantics, as for SQL. Master/user access/refresh tokens are
not project-service authority. Body project IDs/extra fields do not redirect work.
Public data exchange neither checks nor advances private session time/WAL.

Run decoding, physical row checks, recovery and commit on blocking workers. A
started operation retains its scope/permit until it finishes despite cancellation.
Return only count/transaction import metadata after durable commit; export is
explicit plaintext data. Both endpoints disable caches on success and refusal,
including authorization failure. Static error codes distinguish invalid transfer,
body bounds and storage/uncertain write outcome without echoing content or paths.
No new engine, stored format, user role, row policy or whole-process quota is added.

Cover both routers, private/sibling isolation, current-key replacement during body
wait, invalid inputs, exact WAL preservation, output cap, real TCP post-ACK kills
and reopen on WAL1/2. Extend the common native/container lifecycle with one table
import ACK kill (seven total) and no private-history change; drop the verified
temporary public copy before the existing backup/restore scenario. OpenAPI describes
the separate float_bits contract and transport limits. Production gates remain open.
