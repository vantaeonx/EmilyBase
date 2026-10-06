# Experimental physical image plans

The raw synchronous transaction model can now materialize and independently replay
its changed physical components. This is a prerequisite experiment for combined
table/index storage. It has no WAL append, fsync, durable commit or new file format.
The normal server, CLI, backup and existing WAL versions keep their behavior.

A Prepared keeps its exact immutable base. `image_plan()` yields an immutable
ImagePlan containing database identity, base/next transaction and state hashes,
changed history images, changed primary images/root bindings and retired roots.
It omits unchanged root selections and unchanged index pages. All image bodies
come from the original page/index engines; namespaces use the existing standalone
PageAddress contract. Page numbers alone cannot identify an owner/domain.

`replay(&base)` returns a separate raw Model only if it reconstructs the complete
expected next state. It checks the exact base, adjacent transaction, image checksums
and typed addresses, sorted unique changes/retirements, original tree topology and
every current live key/physical pointer. Extension of the last history page keeps
all its old slots byte-identical; new pages are contiguous. Earlier history rewrites
are refused even when the page's repaired checksum is valid. A dropped root binds
its exact old metadata/hash. Neither success nor refusal mutates the supplied base.

```rust
use emilybase_catalog::{Column, DataType, Schema};
use emilybase_commit_model::Model;
use emilybase_database::{Event, EventKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut base = Model::new([7; 16])?;
    let mut stage = base.begin()?;
    stage.apply(Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: "items".into(),
            columns: vec![Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            }],
            primary_key: 0,
        }),
    })?;
    stage.rebuild_index("items")?;
    let prepared = stage.prepare()?;
    let plan = prepared.image_plan()?;
    let replayed = plan.replay(&base)?;
    assert_eq!(base.transaction(), 1);
    base.publish(prepared)?; // memory publication only
    assert_eq!(base.fingerprint(), replayed.fingerprint());
    Ok(())
}
```

| Component | Inclusive count bound |
| --- | ---: |
| Changed last/appended history images | 256 |
| Primary image upserts across changed roots | 2048 |
| Retired primary page IDs across changed roots | 2048 |
| Changed root bindings | 128 |
| Retired table roots | 128 |

PlanCounts reports exact counts and original image-body bytes. The maximum body
sum is `(256 + 2048) * 4096 = 9437184`. This excludes address/root/retirement/fence
framing, full retained states and Rust/allocator overhead; it is not a journal or
heap budget. Each tree still has its independent 1024-page topology bound.

A 10000-row dense arena can readdress all 768 pages in one index-only plan, which
has no history image. The old 256-table-event/page limit remains separate. A second
case stages 256 long-value records as 256 new contiguous history pages. These tests
verify physical replay, old views and current row targets; no disk durability is
implied. Incremental maintenance can have a different arena shape from a dense
rebuild, so the full 768-page test constructs its shape explicitly.

The API is deliberately limited to raw Model/Prepared. ModelPool readers and
prepared leases expose no clonable raw state or plan/replay output. Plan buffers,
caller-owned copies and independently replayed models are outside that pool.
History replay currently rebuilds the entire bounded page/catalog state; index
deltas can materialize full temporary envelopes. Their peak costs and concurrent
replay/worker/backup reservation need further work. No wire decoder or runtime
migration is added. See [ADR 0044](adr/0044-validated-physical-image-plans.md),
[prototype gates](durable-index-prototype-plan.md) and [capacity](durable-index-capacity.md).
