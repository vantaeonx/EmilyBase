//! Admission for retained EBIP payload bytes only, not an allocator/RSS quota.
use std::sync::{Arc, Mutex, MutexGuard};

use super::{IMAGE_PLAN_MAX_BYTES, ImagePlan};
use crate::{Error, Result};

pub const MAX_ENVELOPE_BUFFERS: usize = 4096;
pub const MAX_ENVELOPE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeLimit {
    Buffers,
    Bytes,
}

/// Explicit aggregate byte and object caps. Either zero disables retention.
/// No default chooses a deployment size or reserves decoded-state memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeLimits {
    buffers: usize,
    bytes: u64,
}
impl EnvelopeLimits {
    pub fn new(buffers: usize, bytes: u64) -> Result<Self> {
        if buffers > MAX_ENVELOPE_BUFFERS || bytes > MAX_ENVELOPE_BYTES {
            return Err(Error::EnvelopeConfiguration);
        }
        Ok(Self { buffers, bytes })
    }
    pub fn buffers(self) -> usize {
        self.buffers
    }
    pub fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Reserved live serialized payloads, including an in-flight encode/copy.
/// Vec metadata/capacity rounding, the borrowed source and all replay work are
/// outside this count; external copies made from borrowed bytes are caller-owned.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeUsage {
    pub buffers: usize,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct EnvelopePool {
    shared: Arc<Shared>,
}
struct Shared {
    limits: EnvelopeLimits,
    ledger: Mutex<EnvelopeUsage>,
}
struct Permit {
    shared: Arc<Shared>,
    bytes: u64,
}

/// Non-cloneable owned bytes. Explicit cloning must reserve another full copy.
/// Field order releases the actual vector before advertising its budget as free.
pub struct AdmittedEnvelope {
    bytes: Vec<u8>,
    permit: Permit,
}

impl EnvelopePool {
    pub fn new(limits: EnvelopeLimits) -> Self {
        Self {
            shared: Arc::new(Shared {
                limits,
                ledger: Mutex::new(EnvelopeUsage::default()),
            }),
        }
    }
    pub fn limits(&self) -> EnvelopeLimits {
        self.shared.limits
    }
    pub fn usage(&self) -> Result<EnvelopeUsage> {
        Ok(*self.shared.lock()?)
    }

    /// Reserve one buffer and its complete exact serialized size before encode
    /// allocates the output Vec. The input raw plan has its separate lifetime.
    pub fn encode(&self, plan: &ImagePlan) -> Result<AdmittedEnvelope> {
        let length = plan.counts()?.envelope_bytes()?;
        let permit = self.shared.reserve(length)?;
        let bytes = plan.encode()?;
        if bytes.len() as u64 != length {
            return Err(Error::PlanLength);
        }
        Ok(AdmittedEnvelope { bytes, permit })
    }

    /// Validate the full immutable borrowed envelope before admission/copy.
    /// Preflight allocates no image vectors; per-page decoder scratch is bounded.
    pub fn copy_encoded(&self, bytes: &[u8]) -> Result<AdmittedEnvelope> {
        ImagePlan::inspect_encoded(bytes)?;
        let permit = self.shared.reserve(bytes.len() as u64)?;
        let bytes = copy(bytes)?;
        Ok(AdmittedEnvelope { bytes, permit })
    }
}

impl AdmittedEnvelope {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Uses the same shared pool. No derived Clone or owned Vec conversion can
    /// silently release a reservation while returning its retained payload.
    pub fn try_clone(&self) -> Result<Self> {
        let permit = self.permit.shared.reserve(self.bytes.len() as u64)?;
        let bytes = copy(&self.bytes)?;
        Ok(Self { bytes, permit })
    }
}

fn copy(source: &[u8]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(source.len())
        .map_err(|_| Error::PlanAllocation)?;
    bytes.extend_from_slice(source);
    Ok(bytes)
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, EnvelopeUsage>> {
        self.ledger.lock().map_err(|_| Error::EnvelopePoisoned)
    }
    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Permit> {
        if bytes == 0 || bytes > IMAGE_PLAN_MAX_BYTES as u64 {
            return Err(Error::PlanLength);
        }
        let mut ledger = self.lock()?;
        let buffers = ledger
            .buffers
            .checked_add(1)
            .ok_or(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))?;
        let total = ledger
            .bytes
            .checked_add(bytes)
            .ok_or(Error::EnvelopeAdmission(EnvelopeLimit::Bytes))?;
        if buffers > self.limits.buffers {
            return Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers));
        }
        if total > self.limits.bytes {
            return Err(Error::EnvelopeAdmission(EnvelopeLimit::Bytes));
        }
        *ledger = EnvelopeUsage {
            buffers,
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
        // Public operations remain fail-closed. Private exactly-once release
        // still works during unwinding, with no user work in the critical section.
        let mut ledger = self
            .shared
            .ledger
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ledger.buffers -= 1;
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
        staged.prepare().unwrap().image_plan().unwrap()
    }

    #[test]
    fn failed_encode_releases_its_preallocated_reservation_without_poison() {
        let mut plan = plan();
        let pool = EnvelopePool::new(EnvelopeLimits::new(1, MAX_ENVELOPE_BYTES).unwrap());
        // Public generated plans are validated. A private malformed scope tests
        // cleanup after successful reservation but failed materialization.
        plan.database = [9; 16];
        assert!(pool.encode(&plan).is_err());
        assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
        plan.database = [7; 16];
        let envelope = pool.encode(&plan).unwrap();
        assert_eq!(
            pool.usage().unwrap().bytes,
            envelope.as_bytes().len() as u64
        );
        drop(envelope);
        assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
    }

    #[test]
    fn poisoned_ledger_refuses_new_work_but_all_live_permits_release() {
        let plan = plan();
        let pool = EnvelopePool::new(EnvelopeLimits::new(3, MAX_ENVELOPE_BYTES).unwrap());
        let first = pool.encode(&plan).unwrap();
        let second = first.try_clone().unwrap();
        let shared = Arc::clone(&pool.shared);
        assert!(
            std::thread::spawn(move || {
                let _guard = shared.ledger.lock().unwrap();
                panic!("synthetic internal poison");
            })
            .join()
            .is_err()
        );
        assert!(matches!(pool.usage(), Err(Error::EnvelopePoisoned)));
        assert!(matches!(pool.encode(&plan), Err(Error::EnvelopePoisoned)));
        assert!(matches!(
            pool.copy_encoded(first.as_bytes()),
            Err(Error::EnvelopePoisoned)
        ));
        assert!(matches!(first.try_clone(), Err(Error::EnvelopePoisoned)));
        drop(first);
        drop(second);
        let ledger = pool.shared.ledger.lock().err().unwrap().into_inner();
        assert_eq!(*ledger, EnvelopeUsage::default());
    }

    #[test]
    fn exact_reservation_is_visible_before_any_materialization_and_released_on_unwind() {
        let pool = EnvelopePool::new(EnvelopeLimits::new(1, IMAGE_PLAN_MAX_BYTES as u64).unwrap());
        let permit = pool.shared.reserve(IMAGE_PLAN_MAX_BYTES as u64).unwrap();
        assert_eq!(
            pool.usage().unwrap(),
            EnvelopeUsage {
                buffers: 1,
                bytes: IMAGE_PLAN_MAX_BYTES as u64
            }
        );
        assert!(matches!(
            pool.shared.reserve(1),
            Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))
        ));
        drop(permit);
        let shared = Arc::clone(&pool.shared);
        assert!(
            std::thread::spawn(move || {
                let _permit = shared.reserve(424).unwrap();
                panic!("synthetic failure after reservation");
            })
            .join()
            .is_err()
        );
        assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
        assert!(pool.shared.reserve(IMAGE_PLAN_MAX_BYTES as u64).is_ok());
    }

    #[test]
    fn maximum_serialized_budget_admits_six_full_envelopes_and_refuses_the_seventh() {
        let pool = EnvelopePool::new(EnvelopeLimits::new(8, MAX_ENVELOPE_BYTES).unwrap());
        let mut permits = Vec::new();
        for _ in 0..6 {
            permits.push(pool.shared.reserve(IMAGE_PLAN_MAX_BYTES as u64).unwrap());
        }
        let used = 6 * IMAGE_PLAN_MAX_BYTES as u64;
        assert_eq!(
            pool.usage().unwrap(),
            EnvelopeUsage {
                buffers: 6,
                bytes: used
            }
        );
        assert!(matches!(
            pool.shared.reserve(IMAGE_PLAN_MAX_BYTES as u64),
            Err(Error::EnvelopeAdmission(EnvelopeLimit::Bytes))
        ));
        assert_eq!(pool.usage().unwrap().bytes, used);
        drop(permits);
        assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
        // These are real leases without allocated payloads. This proves atomic
        // preallocation decisions, not six actual maximal replay heap workloads.
    }

    #[test]
    fn last_payload_owns_and_releases_the_actual_shared_ledger() {
        let plan = plan();
        let pool = EnvelopePool::new(EnvelopeLimits::new(2, MAX_ENVELOPE_BYTES).unwrap());
        let weak = Arc::downgrade(&pool.shared);
        let first = pool.encode(&plan).unwrap();
        let last = first.try_clone().unwrap();
        drop(pool);
        drop(first);
        assert!(weak.upgrade().is_some());
        drop(last);
        assert!(weak.upgrade().is_none());
    }
}
