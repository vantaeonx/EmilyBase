# Experimental model lifetime admission

`emilybase-commit-model` now offers `ModelPool`, an optional synchronous coordinator
for synthetic table/index memory states. It writes no files and cannot acknowledge
a durable transaction. Existing managed WAL/server behavior is unchanged.

```rust
use emilybase_commit_model::{AdmissionLimits, ModelPool};

fn main() -> Result<(), emilybase_commit_model::Error> {
    // Example configuration, not a recommended server memory quota.
    let pool = ModelPool::new(AdmissionLimits::new(4, 12, 64, 4)?);
    let project = pool.create([1; 16])?;
    let reader = project.read()?;
    let second_reader = reader.try_clone()?;
    let stage = project.begin()?;
    assert_eq!(pool.usage()?.generations, 2); // current plus reserved future
    assert_eq!(pool.usage()?.readers, 2);
    drop(stage); // discard and release the future generation/writer
    assert_eq!(pool.usage()?.generations, 1);
    drop(project); // readers still retain registration and this generation
    drop(reader);
    drop(second_reader);
    assert_eq!(pool.usage()?.projects, 0);
    Ok(())
}
```

Create reserves an initial generation before constructing it. Begin reserves both
a writer and a future generation before copying any staging metadata. Preparing
keeps both reservations; publishing transfers the future generation into current
state. Old readers keep the old generation charged until its last reference drops.
Readers of the same generation share its slot. Counted reader instances can also
be shared by borrowed references or an Arc; these references keep the same object
alive rather than becoming independently counted readers.

The publication owner provides `read`, `begin` and memory-only `publish`. A stage
provides checked table IDs, borrowed point reads, event application, original index
rebuilding and preparation. Rebuild the changed table before prepare; missing
roots/incorrect coverage retain the original complete-state refusal. Rebuilding an
unknown table or duplicate staged index aborts the entire stage. Prepared objects
provide count/component inspection and borrowed point reads before publication.
Neither a reader nor a stage exposes the underlying clonable Model/Snapshot.

| Resource | Charge | Release |
| --- | --- | --- |
| Project registration | create | last owner/reader/stage/prepared descendant |
| Initial generation | create, before model construction | last state holder |
| Future generation | begin, before staging clone | discard/failure, or last published state holder |
| Reader object | read or try_clone | object drop |
| Writer | begin, atomically with future generation | failure/discard/publication |

Errors distinguish project/generation/reader/global-writer capacity, an active
writer for the same project, duplicate database identity and invalid configuration.
A refused reservation changes no counters and no published state. Limits cannot
be changed beneath live leases. Cloned pool handles use one ledger. A prepared
operation from another pool/project is refused even if its state digest matches.
Dropping a publication owner does not allow the identity to be reused while any
of its descendants remains alive.

Configuration ranges are 1..=128 projects, 1..=4096 generations, 0..=4096 readers
and 0..=4 writers. They can intentionally prevent begin or read. Generation usage
includes placeholders for stages that have not prepared a complete state yet.
These counts bound owned model lifetimes; they are not encoded bytes, heap bytes,
RSS or an estimate from the diagnostic reports. Copies callers make from rows or
schemas, raw models, read/build temporaries, thread stacks and runtime/WAL/replay
buffers remain outside the coordinator. No server admission gate is completed by
this change. See [ADR 0043](adr/0043-bounded-model-lifetimes.md).

The independent `model_admission` ASan sequence target also checks orphaned
registrations, recreation, disabled capacities, prepared publication and complete
release. Its bounded execution evidence is in [testing](testing.md#model-admission-sequence-fuzz-checkpoint).

## Follow-up: retained serialized payload reservation

The optional [EnvelopePool](image-buffer-admission.md) now atomically reserves
complete EBIP byte lengths and buffer slots before encode/copy, including admitted
clones. Admitted preparation can serialize without releasing writer/generation
leases or exposing raw state. Raw plans, decoded/retained model state, temporary
validation and whole-process/server/WAL admission remain outside this scope;
[ADR 0049](adr/0049-admitted-serialized-image-buffers.md) closes no durable gate.
