# ADR 0056: compact owned buffers before publishing index pages

Status: accepted for in-memory index page ownership. EBIX/EBIF formats unchanged.

## Reproduced problem

The owned index API validates text lengths and page arity, but it retained incoming
String/Vec capacity. A three-byte UTF-8/NUL key backed by a 1-MiB String retained
1049016 requested bytes after insertion. Three private constructor/insertion
regressions and a separate native retained guard failed before implementation.
Branch constructors also retained 1024-element key/child reservations for one key
and two children. Leaf unzip and mutation vector growth introduced spare slots;
branch decode grew its child vector geometrically. A bounded wire image therefore
did not establish a predictable retained payload shape.

## Decision

Normalize a validated owned page immediately before immutable retention. Compact
all text keys and key/leaf-pointer/branch-child vectors through owned boxed
strings/slices. Exact reported capacities keep their buffer addresses. This is a
private synchronous helper, with no new dependency, unsafe code or public API.

Public leaf/branch constructors and untrusted page decode validate before this
conversion. Internal allocation and changed-page publication also use it after
the existing operation checks. Semantic equality is tested before changed-page
publication, so unchanged pages retain their original Arc and borrowed addresses.
Dense ID relocation retains equal owners and compacts changed owners. Empty leaf
payloads retain no vector slots. Every retained element type has nonzero size.

Do not alter keys, UTF-8/NUL bytes, ordering, row pointers, links, page IDs, CRCs,
format versions, revision hashes or validation/refusal order. Mutations still
stage their private map before successful publication; old views remain immutable.
The table primary cache and model inherit this page representation but receive no
new numeric byte limit, durable publication or transaction acknowledgement rule.

## Evidence and boundaries

Thirteen private checks include the original three regressions, empty/maximal text,
all branch arities, exact fast-path addresses, invalid typed errors, both ID
policies, split/borrow/merge/root collapse, retired ID reuse, historical owner
release and the full 10000-entry/768-page arena. A 48-case independent mutation
model inspects actual private vector capacities, borrowed keys, rows, page images
and up to four old views. Import, snapshot replay and delta application keep the
same canonical fingerprints and equal-page owners.

Four public checks compare independently sized inputs before serialization, retain
Unicode normalization distinctions, verify failed operations preserve borrowed
addresses and exercise standalone full/delta publication through repeated reopen.
Published frozen version-1 images remain governed by their existing regression.
Standalone index persistence is still separate from the managed table WAL.

The isolated native fixture now retains 323 requested bytes after insertion, and
43 bytes after each leaf/branch construction; operation-local current bytes return
to zero after release. Peaks include deliberately inflated input construction.
See the source-bound counters in ../measurements/2026-10-07-retained-index/.

Reported capacities describe logical payload allocations. They do not bound
allocator usable-size, fragmentation, caller input allocations, transient copying,
map/Arc nodes, process RSS or cold cache construction. Complete numeric model/cache/
staging/replay budgets and the combined durable writer remain open under ADR 0031.
No production or hardware power-loss gate closes with this ownership fix.
