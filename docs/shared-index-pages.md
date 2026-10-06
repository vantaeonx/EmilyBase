# Shared immutable index pages

The original B+ arena now shares immutable decoded pages between tree clones and
historical snapshots. A clone copies the bounded map of handles; unchanged keys
and their pages stay at the same addresses. Writes own private changed pages and
keep old readers valid. See [ADR0051](adr/0051-shared-immutable-index-pages.md).

## Ownership contract

Every arena still owns its page-ID map, root, entry count and stable/dense mode.
Only immutable page bodies share ownership. Replace detaches its target leaf;
insert/delete can detach their route and rebalancing siblings. A visited branch
whose complete value is unchanged retains its old handle. Allocated/reused IDs
receive distinct bodies; historical snapshots with the same ID retain old keys,
pointers and links. The last owner releases the old page.

Insertion/deletion error paths retain exact original values and owner identities.
Delta application borrows the base, copies its handle map, privately decodes changed
pages and performs complete final topology and original physical admission. Empty
deltas share every page but still advance the exact adjacent revision. Wrong
predecessor/digest/count/topology cannot partially change the base. Independent
owned copies decoded from wire are equal but have independent page owners.

Dense deletion still preserves its version-1 dense-ID contract. Renumbering may
change many IDs and child/leaf links, so it may detach many pages. Sharing does not
remove full validation or promise only one allocation for every mutation. The map,
visited-page temporary values, Arc metadata and encoded outputs remain separate
memory costs. Public callers cannot mutate an arena page through a shared handle.

## Measured operation regression

Before changing ownership, the isolated optional native test fails with a clone
peak of3282472 requested bytes. Retained fixtures are built before each operation's
profiler and excluded from the sample. The updated same10000-key,256-byte-text,
768-page fixture records:

| Operation | Requested peak bytes |
| --- | ---: |
| Clone with unchanged page/key bodies | 26304 |
| Full validation / fingerprint | 25376 |
| Encoding, including3149824 output bytes | 3154096 |
| Decode, including independent owned pages | 3285872 |
| Unchanged / one-page delta generation | 25376 |
| One-page delta application with private map | 55912 |
| Four actual workers with separately decoded fixtures | 95704 |

Operation-local current requested bytes return to zero. Clone and application
now use the128-KiB guard alongside validation/hash/delta; decode retains the output
size plus1-MiB guard, encoding its output plus128 KiB, and four workers512 KiB.
The allocator belongs only to the feature-gated native test process. These guards
cover this workload/toolchain; they are not whole-process or worst-case quotas.

## Whole-model observations

Three preserved version-3 reports use the exact same config: index-replay,
short-text keys,10000 rows, four projects,768 value bytes, retained old views and
four actual scoped replay workers. Construction/staging is serial; only replay
uses the parallel workers. Each project retains3334 history pages and792 index
pages; the root-only plans have no physical history/index upserts. Complete state,
fingerprint and selected-pointer verification execute before final release.

| Implementation | Requested peak bytes | Requested bytes cumulatively allocated |
| --- | ---: | ---: |
| Before streamed admission,6ae2ebf | 196255216 | 144137749368 |
| Streamed admission,b9237d5 | 172314032 | 142777335992 |
| Shared immutable pages | 163075408 | 10046597208 |

The shared-page replay retains159074408 current bytes against158854040 before
replay:220368 additional bytes in this observation. This includes its private maps
and other model/worker results. The earlier streamed replay retains13278352 bytes
more than its plan phase; exact original counters are preserved rather than used
as a numeric admission policy. All three final reports retain1016 bytes/four blocks,
matching the separately investigated coordinator/runtime allocation residue;
pages do not become a zero-byte whole-process claim.

Native instrumented elapsed observations are577.51,595.18 and35.81 seconds;
maximum process RSS is263760,225700 and205852 KiB. Profiling overhead and machine
scheduling dominate some costs; these elapsed observations are not database
throughput or deployment capacity claims. Source hashes bind the current index
implementation, and the earlier post-stream executable's source binding was
checked against b9237d5 before preserving its output.

[Reports and observation metadata](measurements/2026-10-06-shared-index-pages/observations.json)
contain counters/configuration/source hashes only. The strict8192-byte report
decoder checks all three raw report fixtures in tests. No stack trace, host path,
credential or actual user row is published.

## Compatibility and remaining gates

Private tests observe actual owner identity, key addresses and last-owner release.
Generated mixed integer/text/NUL histories preserve retained generations through
accepted/discarded mutations and replay. Root collapse, sparse holes, ID reuse,
dense remapping, exhausted-arena atomic failure and four independent writers run.
Model tests cover unchanged-root serialization/replay, one-leaf pointer changes,
stale publication and retained output after source/plan release. Lifetime fuzzing
additionally retains several independently modelled historical views and drops
current state before verifying them.

EBIF/EBIX/EBIP bytes and frozen hashes stay unchanged. Stored database pages,
WAL1/2, backups and commit acknowledgments keep their existing meanings. Exported
relational primary trees still pass their current complete owned export path;
this increment does not silently change that API's admission. Numeric decoded
state/transient/worker quotas and the combined durable writer remain open.
