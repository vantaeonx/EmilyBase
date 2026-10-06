//! Bounded lifetimes for the experimental memory model, not a heap allocator.
//! Raw Model/Snapshot values and caller-owned query results are outside this pool.
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use emilybase_catalog::{Key, Row, Schema};
use emilybase_commit_format::{DatabaseId, RootBinding};
use emilybase_database::{Event, RowLocation};

use crate::{
    AdmittedEnvelope, EncodedComponents, EnvelopePool, Error, Model, Prepared, Result, Staged,
};

pub const MAX_LIFETIME_SLOTS: usize = 4096;
pub const MAX_MODEL_WRITERS: usize = 4;
pub const MAX_MODEL_PROJECTS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionLimit {
    Projects,
    Generations,
    Readers,
    Writers,
    ProjectWriter,
}

/// Counts, not bytes. A pending writer reserves a full generation slot before
/// copying its base metadata. Zero readers/writers deliberately disables access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionLimits {
    projects: usize,
    generations: usize,
    readers: usize,
    writers: usize,
}

impl AdmissionLimits {
    pub fn new(
        projects: usize,
        generations: usize,
        readers: usize,
        writers: usize,
    ) -> Result<Self> {
        if !(1..=MAX_MODEL_PROJECTS).contains(&projects)
            || !(1..=MAX_LIFETIME_SLOTS).contains(&generations)
            || readers > MAX_LIFETIME_SLOTS
            || writers > MAX_MODEL_WRITERS
        {
            return Err(Error::AdmissionConfiguration);
        }
        Ok(Self {
            projects,
            generations,
            readers,
            writers,
        })
    }

    pub fn projects(self) -> usize {
        self.projects
    }
    pub fn generations(self) -> usize {
        self.generations
    }
    pub fn readers(self) -> usize {
        self.readers
    }
    pub fn writers(self) -> usize {
        self.writers
    }
}

/// Includes current/retained generations and the future generation of each
/// active stage/prepared operation. Equal-state readers share one generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionUsage {
    pub projects: usize,
    pub generations: usize,
    pub readers: usize,
    pub writers: usize,
}

#[derive(Clone)]
pub struct ModelPool {
    shared: Arc<Shared>,
}

struct Shared {
    limits: AdmissionLimits,
    ledger: Mutex<Ledger>,
}

#[derive(Default)]
struct Ledger {
    projects: BTreeSet<DatabaseId>,
    writers: BTreeSet<DatabaseId>,
    generations: usize,
    readers: usize,
}

struct Owner {
    shared: Arc<Shared>,
    database: DatabaseId,
}

enum Kind {
    Generation,
    Reader,
    Writer,
}

/// Non-cloneable private token. Every token is constructed only after one
/// atomic ledger reservation; dropping it releases exactly that reservation.
struct Lease {
    owner: Arc<Owner>,
    kind: Kind,
}

struct Generation {
    // Data drops before the lease: a freed slot cannot precede its actual state.
    model: Model,
    lease: Lease,
}

/// One mutable publication owner; its identity stays registered while any
/// reader/stage/prepared descendant is alive, even after this owner is dropped.
pub struct ModelProject {
    current: Arc<Generation>,
}

/// Borrowed reads only. No raw Model, Snapshot or mutable selection escapes.
/// Copies made from an individual Row/Schema are caller-owned, outside the pool.
pub struct ModelReader {
    generation: Arc<Generation>,
    lease: Lease,
}

pub struct AdmittedStage {
    inner: Staged,
    base: Arc<Generation>,
    writer: Lease,
    generation: Lease,
}

pub struct AdmittedPrepared {
    inner: Prepared,
    base: Arc<Generation>,
    writer: Lease,
    generation: Lease,
}

impl ModelPool {
    pub fn new(limits: AdmissionLimits) -> Self {
        Self {
            shared: Arc::new(Shared {
                limits,
                ledger: Mutex::new(Ledger::default()),
            }),
        }
    }

    pub fn limits(&self) -> AdmissionLimits {
        self.shared.limits
    }

    pub fn usage(&self) -> Result<AdmissionUsage> {
        let ledger = self.shared.lock()?;
        Ok(AdmissionUsage {
            projects: ledger.projects.len(),
            generations: ledger.generations,
            readers: ledger.readers,
            writers: ledger.writers.len(),
        })
    }

    /// Reserve registration and its initial generation before constructing a
    /// synthetic empty model. No existing externally cloned state is imported.
    pub fn create(&self, database: DatabaseId) -> Result<ModelProject> {
        {
            let mut ledger = self.shared.lock()?;
            if ledger.projects.contains(&database) {
                return Err(Error::DuplicateDatabase);
            }
            if ledger.projects.len() >= self.shared.limits.projects {
                return Err(Error::Admission(AdmissionLimit::Projects));
            }
            if ledger.generations >= self.shared.limits.generations {
                return Err(Error::Admission(AdmissionLimit::Generations));
            }
            ledger.projects.insert(database);
            ledger.generations += 1;
        }
        let owner = Arc::new(Owner {
            shared: Arc::clone(&self.shared),
            database,
        });
        let lease = Lease {
            owner,
            kind: Kind::Generation,
        };
        let model = Model::new(database)?;
        Ok(ModelProject {
            current: Arc::new(Generation { model, lease }),
        })
    }
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, Ledger>> {
        self.ledger.lock().map_err(|_| Error::AdmissionPoisoned)
    }
}

impl Owner {
    fn reader(self: &Arc<Self>) -> Result<Lease> {
        let mut ledger = self.shared.lock()?;
        if ledger.readers >= self.shared.limits.readers {
            return Err(Error::Admission(AdmissionLimit::Readers));
        }
        ledger.readers += 1;
        Ok(Lease {
            owner: Arc::clone(self),
            kind: Kind::Reader,
        })
    }

    fn writer(self: &Arc<Self>) -> Result<(Lease, Lease)> {
        let mut ledger = self.shared.lock()?;
        if ledger.writers.contains(&self.database) {
            return Err(Error::Admission(AdmissionLimit::ProjectWriter));
        }
        if ledger.writers.len() >= self.shared.limits.writers {
            return Err(Error::Admission(AdmissionLimit::Writers));
        }
        if ledger.generations >= self.shared.limits.generations {
            return Err(Error::Admission(AdmissionLimit::Generations));
        }
        ledger.writers.insert(self.database);
        ledger.generations += 1;
        Ok((
            Lease {
                owner: Arc::clone(self),
                kind: Kind::Writer,
            },
            Lease {
                owner: Arc::clone(self),
                kind: Kind::Generation,
            },
        ))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Release must run even during unwinding. The critical section calls no
        // user code; poisoned operations remain refused by Shared::lock.
        let mut ledger = self
            .owner
            .shared
            .ledger
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        match self.kind {
            Kind::Generation => ledger.generations -= 1,
            Kind::Reader => ledger.readers -= 1,
            Kind::Writer => {
                ledger.writers.remove(&self.owner.database);
            }
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let mut ledger = self
            .shared
            .ledger
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        ledger.projects.remove(&self.database);
    }
}

impl ModelProject {
    pub fn database_id(&self) -> DatabaseId {
        self.current.model.database_id()
    }
    pub fn transaction(&self) -> u64 {
        self.current.model.transaction()
    }
    pub fn fingerprint(&self) -> [u8; 32] {
        self.current.model.fingerprint()
    }

    pub fn read(&self) -> Result<ModelReader> {
        let lease = self.current.lease.owner.reader()?;
        Ok(ModelReader {
            generation: Arc::clone(&self.current),
            lease,
        })
    }

    /// Global writer/generation admission and per-project exclusion happen as
    /// one decision before Staged::new clones any relational metadata.
    pub fn begin(&self) -> Result<AdmittedStage> {
        let (writer, generation) = self.current.lease.owner.writer()?;
        let inner = self.current.model.begin()?;
        Ok(AdmittedStage {
            inner,
            base: Arc::clone(&self.current),
            writer,
            generation,
        })
    }

    /// Memory-only publication. Instance identity rejects foreign pools and
    /// recreated owners even if their database ID and fingerprint are equal.
    pub fn publish(&mut self, prepared: AdmittedPrepared) -> Result<()> {
        if !Arc::ptr_eq(&self.current, &prepared.base) {
            return Err(Error::Conflict);
        }
        let AdmittedPrepared {
            inner,
            base,
            writer,
            generation,
        } = prepared;
        let mut model = base.model.clone();
        model.publish(inner)?;
        self.current = Arc::new(Generation {
            model,
            lease: generation,
        });
        drop(writer);
        drop(base);
        Ok(())
    }
}

impl ModelReader {
    /// Every clone is admitted; a refused clone leaves the original usable.
    pub fn try_clone(&self) -> Result<Self> {
        let lease = self.lease.owner.reader()?;
        Ok(Self {
            generation: Arc::clone(&self.generation),
            lease,
        })
    }
    pub fn database_id(&self) -> DatabaseId {
        self.generation.model.database_id()
    }
    pub fn transaction(&self) -> u64 {
        self.generation.model.transaction()
    }
    pub fn fingerprint(&self) -> [u8; 32] {
        self.generation.model.fingerprint()
    }
    pub fn row_count(&self) -> usize {
        self.generation.model.view().row_count()
    }
    pub fn encoded_components(&self) -> Result<EncodedComponents> {
        self.generation.model.encoded_components()
    }
    pub fn table_id(&self, name: &str) -> Result<u64> {
        Ok(self.generation.model.view().table_id(name)?)
    }
    pub fn schema(&self, name: &str) -> Result<&Schema> {
        Ok(self.generation.model.view().schema(name)?)
    }
    pub fn get(&self, name: &str, key: &Key) -> Result<Option<&Row>> {
        Ok(self.generation.model.view().get(name, key)?)
    }
    pub fn row_location(&self, name: &str, key: &Key) -> Result<Option<RowLocation>> {
        Ok(self.generation.model.view().row_location(name, key)?)
    }
    pub fn binding(&self, table: u64) -> Option<RootBinding> {
        self.generation
            .model
            .selection(table)
            .map(|selection| selection.binding())
    }
}

impl AdmittedStage {
    pub fn transaction(&self) -> u64 {
        self.inner.transaction()
    }
    pub fn next_table_id(&self) -> Result<u64> {
        Ok(self.inner.view()?.next_table_id())
    }
    pub fn table_id(&self, name: &str) -> Result<u64> {
        Ok(self.inner.view()?.table_id(name)?)
    }
    pub fn row_count(&self) -> Result<usize> {
        Ok(self.inner.view()?.row_count())
    }
    pub fn get(&self, name: &str, key: &Key) -> Result<Option<&Row>> {
        Ok(self.inner.view()?.get(name, key)?)
    }
    pub fn apply(&mut self, event: Event) -> Result<()> {
        self.inner.apply(event)
    }
    pub fn rebuild_index(&mut self, name: &str) -> Result<()> {
        self.inner.rebuild_index(name)
    }
    pub fn prepare(self) -> Result<AdmittedPrepared> {
        let Self {
            inner,
            base,
            writer,
            generation,
        } = self;
        let inner = inner.prepare()?;
        Ok(AdmittedPrepared {
            inner,
            base,
            writer,
            generation,
        })
    }
}

impl AdmittedPrepared {
    /// Own only admitted serialized bytes. The temporary raw physical plan is
    /// outside this byte pool; no clonable Model or Snapshot escapes.
    pub fn encode_in(&self, pool: &EnvelopePool) -> Result<AdmittedEnvelope> {
        pool.encode(&self.inner.image_plan()?)
    }

    pub fn transaction(&self) -> u64 {
        self.inner.transaction()
    }
    pub fn row_count(&self) -> usize {
        self.inner.view().row_count()
    }
    pub fn encoded_components(&self) -> Result<EncodedComponents> {
        self.inner.encoded_components()
    }
    pub fn get(&self, name: &str, key: &Key) -> Result<Option<&Row>> {
        Ok(self.inner.view().get(name, key)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_admission_refuses_operations_but_drops_still_release_the_ledger() {
        let pool = ModelPool::new(AdmissionLimits::new(1, 2, 2, 1).unwrap());
        let project = pool.create([1; 16]).unwrap();
        let reader = project.read().unwrap();
        let stage = project.begin().unwrap();
        let shared = Arc::clone(&pool.shared);
        assert!(
            std::thread::spawn(move || {
                let _guard = shared.ledger.lock().unwrap();
                panic!("synthetic internal poison");
            })
            .join()
            .is_err()
        );
        assert!(matches!(pool.usage(), Err(Error::AdmissionPoisoned)));
        assert!(matches!(project.read(), Err(Error::AdmissionPoisoned)));
        assert!(matches!(project.begin(), Err(Error::AdmissionPoisoned)));
        assert!(matches!(
            pool.create([2; 16]),
            Err(Error::AdmissionPoisoned)
        ));
        drop(stage);
        drop(project);
        drop(reader);
        let ledger = pool.shared.ledger.lock().err().unwrap().into_inner();
        assert!(ledger.projects.is_empty());
        assert!(ledger.writers.is_empty());
        assert_eq!((ledger.generations, ledger.readers), (0, 0));
    }

    #[test]
    fn actual_generation_storage_drops_with_its_last_reader_before_slot_reuse() {
        let pool = ModelPool::new(AdmissionLimits::new(1, 1, 2, 0).unwrap());
        let project = pool.create([1; 16]).unwrap();
        let state = Arc::downgrade(&project.current.model.state);
        let reader = project.read().unwrap();
        let generation = Arc::downgrade(&project.current);
        drop(project);
        assert!(generation.upgrade().is_some());
        assert!(state.upgrade().is_some());
        assert_eq!(pool.usage().unwrap().generations, 1);
        drop(reader);
        assert!(generation.upgrade().is_none());
        assert!(state.upgrade().is_none());
        assert_eq!(pool.usage().unwrap().generations, 0);
        assert!(pool.create([1; 16]).is_ok());
    }
}
