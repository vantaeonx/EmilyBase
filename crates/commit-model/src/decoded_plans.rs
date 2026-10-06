//! Exact retained vector-payload admission, not a model/allocator/RSS quota.
use std::mem::size_of;
use std::sync::{Arc, Mutex, MutexGuard};

use super::{
    AdmittedEnvelope, ImagePlan, PageAddress, PageWrite, PlanCounts, RetiredRoot, RootChange,
};
use crate::{Error, Result};

pub const MAX_DECODED_PLANS: usize = 4096;
pub const MAX_DECODED_PLAN_BYTES: u64 = 64 * 1024 * 1024;
/// Host-layout vector payload bound, not serialized EBIP bytes or allocator overhead.
pub const MAX_DECODED_PLAN_VECTOR_BYTES: u64 =
    (crate::MAX_EVENTS + crate::MAX_SELECTED_INDEX_PAGES) as u64 * size_of::<PageWrite>() as u64
        + crate::MAX_SELECTED_INDEX_PAGES as u64 * size_of::<PageAddress>() as u64
        + emilybase_database::MAX_TABLES as u64
            * (size_of::<RootChange>() + size_of::<RetiredRoot>()) as u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedPlanLimit {
    Plans,
    Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedPlanLimits {
    plans: usize,
    bytes: u64,
}
impl DecodedPlanLimits {
    /// Either zero disables new decoded retention. No implicit deployment size.
    pub fn new(plans: usize, bytes: u64) -> Result<Self> {
        if plans > MAX_DECODED_PLANS || bytes > MAX_DECODED_PLAN_BYTES {
            return Err(Error::DecodedConfiguration);
        }
        Ok(Self { plans, bytes })
    }
    pub fn plans(self) -> usize {
        self.plans
    }
    pub fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Includes pending decodes; shared handles charge one physical vector owner.
/// Arc/plan inline metadata, source bytes, scratch/replay/models are excluded.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DecodedPlanUsage {
    pub plans: usize,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct DecodedPlanPool {
    shared: Arc<Shared>,
}
struct Shared {
    limits: DecodedPlanLimits,
    ledger: Mutex<DecodedPlanUsage>,
}
struct Permit {
    shared: Arc<Shared>,
    bytes: u64,
}

/// Immutable shared owner. Clone retains the same vectors and reservation.
/// No mutable plan or owned conversion can release its charge prematurely.
#[derive(Clone)]
pub struct AdmittedPlan {
    inner: Arc<Inner>,
}
struct Inner {
    // Rust field order frees all actual vectors before advertising their bytes.
    plan: ImagePlan,
    permit: Permit,
}

impl PlanCounts {
    /// Requested payload for all owned EBIP decoded vectors, including typed
    /// address/root metadata. Values use this binary's Rust type layout, not a
    /// portable file field. Complete state/topology replay remains mandatory.
    pub fn decoded_vector_bytes(self) -> Result<u64> {
        payload([
            (
                self.history.checked_add(self.primary).ok_or(Error::Limit)?,
                size_of::<PageWrite>(),
            ),
            (self.retired_pages, size_of::<PageAddress>()),
            (self.changed_roots, size_of::<RootChange>()),
            (self.retired_tables, size_of::<RetiredRoot>()),
        ])
    }
}

impl DecodedPlanPool {
    pub fn new(limits: DecodedPlanLimits) -> Self {
        Self {
            shared: Arc::new(Shared {
                limits,
                ledger: Mutex::new(DecodedPlanUsage::default()),
            }),
        }
    }
    pub fn limits(&self) -> DecodedPlanLimits {
        self.shared.limits
    }
    pub fn usage(&self) -> Result<DecodedPlanUsage> {
        Ok(*self.shared.lock()?)
    }

    /// Complete borrowed structural preflight precedes reservation. Reservation
    /// precedes owned image-vector construction. It does not authorize a base.
    pub fn decode(&self, bytes: &[u8]) -> Result<AdmittedPlan> {
        let counts = ImagePlan::inspect_encoded(bytes)?;
        let length = counts.decoded_vector_bytes()?;
        let permit = self.shared.reserve(length)?;
        let plan = ImagePlan::decode(bytes)?;
        // No successful object may retain hidden spare vector capacity outside
        // its charge. Unexpected allocator/layout behavior refuses publication;
        // all private vectors and the permit are dropped on that path.
        retain(Inner { plan, permit })
    }
}

impl AdmittedPlan {
    pub fn plan(&self) -> &ImagePlan {
        &self.inner.plan
    }
    pub fn reserved_vector_bytes(&self) -> u64 {
        self.inner.permit.bytes
    }
}

impl AdmittedEnvelope {
    /// The encoded source keeps its own independent serialized reservation.
    pub fn decode_in(&self, pool: &DecodedPlanPool) -> Result<AdmittedPlan> {
        pool.decode(self.as_bytes())
    }
}

fn retain(inner: Inner) -> Result<AdmittedPlan> {
    if capacity_bytes(&inner.plan)? != inner.permit.bytes {
        return Err(Error::DecodedAllocationShape);
    }
    Ok(AdmittedPlan {
        inner: Arc::new(inner),
    })
}

fn payload<const N: usize>(parts: [(usize, usize); N]) -> Result<u64> {
    parts.into_iter().try_fold(0u64, |total, (count, width)| {
        let bytes = (count as u64)
            .checked_mul(width as u64)
            .ok_or(Error::Limit)?;
        total.checked_add(bytes).ok_or(Error::Limit)
    })
}

fn capacity_bytes(plan: &ImagePlan) -> Result<u64> {
    let mut bytes = payload([
        (plan.history.capacity(), size_of::<PageWrite>()),
        (plan.roots.capacity(), size_of::<RootChange>()),
        (plan.retired.capacity(), size_of::<RetiredRoot>()),
    ])?;
    for root in &plan.roots {
        bytes = bytes
            .checked_add(payload([
                (root.upserts.capacity(), size_of::<PageWrite>()),
                (root.retired.capacity(), size_of::<PageAddress>()),
            ])?)
            .ok_or(Error::Limit)?;
    }
    Ok(bytes)
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, DecodedPlanUsage>> {
        self.ledger.lock().map_err(|_| Error::DecodedPoisoned)
    }
    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Permit> {
        if bytes == 0 || bytes > MAX_DECODED_PLAN_VECTOR_BYTES {
            return Err(Error::PlanLength);
        }
        let mut ledger = self.lock()?;
        let plans = ledger
            .plans
            .checked_add(1)
            .ok_or(Error::DecodedAdmission(DecodedPlanLimit::Plans))?;
        if plans > self.limits.plans {
            return Err(Error::DecodedAdmission(DecodedPlanLimit::Plans));
        }
        let total = ledger
            .bytes
            .checked_add(bytes)
            .ok_or(Error::DecodedAdmission(DecodedPlanLimit::Bytes))?;
        if total > self.limits.bytes {
            return Err(Error::DecodedAdmission(DecodedPlanLimit::Bytes));
        }
        *ledger = DecodedPlanUsage {
            plans,
            bytes: total,
        };
        Ok(Permit {
            shared: Arc::clone(self),
            bytes,
        })
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        // Cleanup remains exactly-once during unwinding and after internal poison.
        // New reservations still fail closed; no user work runs under this lock.
        let mut ledger = self
            .shared
            .ledger
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ledger.plans -= 1;
        ledger.bytes -= self.bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model;
    use emilybase_catalog::{Column, DataType, Schema};
    use emilybase_database::{Event, EventKind};

    fn plan() -> ImagePlan {
        let base = Model::new([7; 16]).unwrap();
        let mut staged = base.begin().unwrap();
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Create(Schema {
                    name: "items".into(),
                    primary_key: 0,
                    columns: vec![Column {
                        name: "id".into(),
                        data_type: DataType::Integer,
                        nullable: false,
                    }],
                }),
            })
            .unwrap();
        staged.rebuild_index("items").unwrap();
        let encoded = staged
            .prepare()
            .unwrap()
            .image_plan()
            .unwrap()
            .encode()
            .unwrap();
        ImagePlan::decode(&encoded).unwrap()
    }

    #[test]
    fn unexpected_spare_capacity_refuses_and_releases_the_whole_private_owner() {
        let mut plan = plan();
        let bytes = plan.counts().unwrap().decoded_vector_bytes().unwrap();
        let pool = DecodedPlanPool::new(DecodedPlanLimits::new(1, MAX_DECODED_PLAN_BYTES).unwrap());
        let permit = pool.shared.reserve(bytes).unwrap();
        assert_eq!(pool.usage().unwrap(), DecodedPlanUsage { plans: 1, bytes });
        // A public decode reserves exact vector lengths. Force a private shape
        // regression to prove that hidden unused payload cannot be published.
        plan.history.reserve(5);
        assert!(capacity_bytes(&plan).unwrap() > bytes);
        assert!(matches!(
            retain(Inner { plan, permit }),
            Err(Error::DecodedAllocationShape)
        ));
        assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
    }

    #[test]
    fn poisoned_ledger_refuses_new_decode_but_all_shared_handles_release() {
        let bytes = plan().encode().unwrap();
        let pool = DecodedPlanPool::new(DecodedPlanLimits::new(1, MAX_DECODED_PLAN_BYTES).unwrap());
        let admitted = pool.decode(&bytes).unwrap();
        let last = admitted.clone();
        let shared = Arc::clone(&pool.shared);
        assert!(
            std::thread::spawn(move || {
                let _guard = shared.ledger.lock().unwrap();
                panic!("synthetic internal poison");
            })
            .join()
            .is_err()
        );
        assert!(matches!(pool.usage(), Err(Error::DecodedPoisoned)));
        assert!(matches!(pool.decode(&bytes), Err(Error::DecodedPoisoned)));
        let clone = last.clone();
        assert!(std::ptr::eq(admitted.plan(), clone.plan()));
        drop(admitted);
        drop(last);
        drop(clone);
        let ledger = pool.shared.ledger.lock().err().unwrap().into_inner();
        assert_eq!(*ledger, DecodedPlanUsage::default());
    }

    #[test]
    fn pending_reservation_releases_exactly_once_during_unwind() {
        let pool = DecodedPlanPool::new(DecodedPlanLimits::new(1, MAX_DECODED_PLAN_BYTES).unwrap());
        let shared = Arc::clone(&pool.shared);
        assert!(
            std::thread::spawn(move || {
                let _permit = shared.reserve(MAX_DECODED_PLAN_VECTOR_BYTES).unwrap();
                panic!("synthetic failure after reservation");
            })
            .join()
            .is_err()
        );
        assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
        assert!(pool.shared.reserve(MAX_DECODED_PLAN_VECTOR_BYTES).is_ok());
    }

    #[test]
    fn maximum_host_payload_permits_fit_exact_aggregate_without_allocating_maximal_plans() {
        let counts = PlanCounts::from_counts(
            crate::MAX_EVENTS,
            crate::MAX_SELECTED_INDEX_PAGES,
            crate::MAX_SELECTED_INDEX_PAGES,
            emilybase_database::MAX_TABLES,
            emilybase_database::MAX_TABLES,
        )
        .unwrap();
        assert_eq!(
            counts.decoded_vector_bytes().unwrap(),
            MAX_DECODED_PLAN_VECTOR_BYTES
        );
        let pool =
            DecodedPlanPool::new(DecodedPlanLimits::new(32, MAX_DECODED_PLAN_BYTES).unwrap());
        let capacity = MAX_DECODED_PLAN_BYTES / MAX_DECODED_PLAN_VECTOR_BYTES;
        let permits: Vec<_> = (0..capacity)
            .map(|_| pool.shared.reserve(MAX_DECODED_PLAN_VECTOR_BYTES).unwrap())
            .collect();
        assert!(matches!(
            pool.shared.reserve(MAX_DECODED_PLAN_VECTOR_BYTES),
            Err(Error::DecodedAdmission(DecodedPlanLimit::Bytes))
        ));
        assert_eq!(
            pool.usage().unwrap().bytes,
            capacity * MAX_DECODED_PLAN_VECTOR_BYTES
        );
        drop(permits);
        assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
        assert!(payload([(usize::MAX, usize::MAX)]).is_err());
    }

    #[test]
    fn last_decoded_owner_retains_and_releases_the_actual_pool_ledger() {
        let bytes = plan().encode().unwrap();
        let pool = DecodedPlanPool::new(DecodedPlanLimits::new(1, MAX_DECODED_PLAN_BYTES).unwrap());
        let weak = Arc::downgrade(&pool.shared);
        let admitted = pool.decode(&bytes).unwrap();
        let last = admitted.clone();
        drop(pool);
        drop(admitted);
        assert!(weak.upgrade().is_some());
        drop(last);
        assert!(weak.upgrade().is_none());
    }
}
