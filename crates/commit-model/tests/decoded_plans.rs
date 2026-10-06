use emilybase_catalog::{DataType, Value};
use emilybase_commit_model::{
    AdmittedPlan, DecodedPlanLimit, DecodedPlanLimits, DecodedPlanPool, DecodedPlanUsage,
    EnvelopeLimits, EnvelopePool, EnvelopeUsage, Error, IMAGE_PLAN_MAX_BYTES, ImagePlan,
    MAX_DECODED_PLAN_BYTES, MAX_DECODED_PLANS, Model, PlanCounts,
};
use emilybase_database::{Event, EventKind};
use proptest::prelude::*;
use std::sync::{Arc, Barrier};
mod support;

fn plans() -> (Model, Vec<u8>, Vec<u8>) {
    let base = support::model(&[("items", DataType::Integer)]);
    let mut stage = base.begin().unwrap();
    stage.rebuild_index("items").unwrap();
    let small = stage
        .prepare()
        .unwrap()
        .image_plan()
        .unwrap()
        .encode()
        .unwrap();
    let mut stage = base.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(7), Value::Text("synthetic".into())]),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    let large = stage
        .prepare()
        .unwrap()
        .image_plan()
        .unwrap()
        .encode()
        .unwrap();
    (base, small, large)
}
fn pool(plans: usize, bytes: u64) -> DecodedPlanPool {
    DecodedPlanPool::new(DecodedPlanLimits::new(plans, bytes).unwrap())
}
fn charge(bytes: &[u8]) -> u64 {
    ImagePlan::inspect_encoded(bytes)
        .unwrap()
        .decoded_vector_bytes()
        .unwrap()
}
fn verifies(owner: &AdmittedPlan, base: &Model, original: &[u8]) {
    assert_eq!(owner.plan().encode().unwrap(), original);
    assert_eq!(owner.reserved_vector_bytes(), charge(original));
    assert_eq!(
        owner.plan().replay(base).unwrap().fingerprint(),
        owner.plan().next_fingerprint()
    );
}

#[test]
fn explicit_bounds_and_disabled_retention_make_no_partial_charge() {
    assert!(DecodedPlanLimits::new(MAX_DECODED_PLANS, MAX_DECODED_PLAN_BYTES).is_ok());
    for (plans, bytes) in [
        (MAX_DECODED_PLANS + 1, 0),
        (usize::MAX, 0),
        (1, MAX_DECODED_PLAN_BYTES + 1),
        (1, u64::MAX),
    ] {
        assert!(matches!(
            DecodedPlanLimits::new(plans, bytes),
            Err(Error::DecodedConfiguration)
        ));
    }
    let (_, small, _) = plans();
    for (plans, bytes, expected) in [
        (0, MAX_DECODED_PLAN_BYTES, DecodedPlanLimit::Plans),
        (1, 0, DecodedPlanLimit::Bytes),
        (0, 0, DecodedPlanLimit::Plans),
    ] {
        let pool = pool(plans, bytes);
        assert_eq!(pool.limits().plans(), plans);
        assert_eq!(pool.limits().bytes(), bytes);
        assert!(
            matches!(pool.decode(&small), Err(Error::DecodedAdmission(limit)) if limit == expected)
        );
        assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
    }
}

#[test]
fn exact_vector_bytes_admit_and_one_byte_less_refuses_before_owned_decode() {
    let (base, small, _) = plans();
    let bytes = charge(&small);
    assert_eq!(
        bytes,
        std::mem::size_of::<emilybase_commit_model::RootChange>() as u64
    );
    assert_ne!(bytes, small.len() as u64);
    let short = pool(1, bytes - 1);
    assert!(matches!(
        short.decode(&small),
        Err(Error::DecodedAdmission(DecodedPlanLimit::Bytes))
    ));
    assert_eq!(short.usage().unwrap(), DecodedPlanUsage::default());
    let exact = pool(1, bytes);
    let owner = exact.decode(&small).unwrap();
    verifies(&owner, &base, &small);
    assert_eq!(exact.usage().unwrap(), DecodedPlanUsage { plans: 1, bytes });
    assert!(matches!(
        exact.decode(&small),
        Err(Error::DecodedAdmission(DecodedPlanLimit::Plans))
    ));
    drop(owner);
    assert_eq!(exact.usage().unwrap(), DecodedPlanUsage::default());
}

#[test]
fn shared_handles_preserve_identity_and_only_last_drop_reclaims_vectors() {
    let (base, _, large) = plans();
    let pool = pool(1, charge(&large));
    let owner = pool.decode(&large).unwrap();
    let clone = owner.clone();
    assert!(std::ptr::eq(owner.plan(), clone.plan()));
    assert!(std::ptr::eq(
        owner.plan().history()[0].image(),
        clone.plan().history()[0].image()
    ));
    assert_eq!(pool.usage().unwrap().plans, 1);
    drop(owner);
    verifies(&clone, &base, &large);
    assert_eq!(pool.usage().unwrap().plans, 1);
    assert!(pool.decode(&large).is_err());
    drop(clone);
    assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
    assert!(pool.decode(&large).is_ok());
}

#[test]
fn independent_decodes_charge_independent_page_bodies_and_mixed_sizes() {
    let (base, small, large) = plans();
    let s = charge(&small);
    let l = charge(&large);
    assert!(l > s);
    let pool = pool(4, s + 2 * l);
    let small_owner = pool.decode(&small).unwrap();
    let first = pool.decode(&large).unwrap();
    let second = pool.decode(&large).unwrap();
    assert!(!std::ptr::eq(first.plan(), second.plan()));
    assert!(!std::ptr::eq(
        first.plan().history()[0].image(),
        second.plan().history()[0].image()
    ));
    assert_eq!(
        pool.usage().unwrap(),
        DecodedPlanUsage {
            plans: 3,
            bytes: s + 2 * l
        }
    );
    assert!(matches!(
        pool.decode(&small),
        Err(Error::DecodedAdmission(DecodedPlanLimit::Bytes))
    ));
    drop(first);
    assert_eq!(
        pool.usage().unwrap(),
        DecodedPlanUsage {
            plans: 2,
            bytes: s + l
        }
    );
    verifies(&second, &base, &large);
    drop(small_owner);
    drop(second);
    assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
}

#[test]
fn full_preflight_refuses_corruption_without_consuming_a_disabled_pool() {
    let (_, small, large) = plans();
    let disabled = pool(0, 0);
    for bytes in [&small, &large] {
        for offset in [0, 8, 16, 112, 132, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[offset] ^= 1;
            let error = disabled.decode(&bad).err().unwrap();
            assert!(!matches!(error, Error::DecodedAdmission(_)));
            assert_eq!(disabled.usage().unwrap(), DecodedPlanUsage::default());
        }
        for end in [0, 1, 191, bytes.len() - 1] {
            assert!(disabled.decode(&bytes[..end]).is_err());
        }
    }
    assert!(matches!(
        disabled.decode(&vec![0; IMAGE_PLAN_MAX_BYTES + 1]),
        Err(Error::PlanLength)
    ));
}

#[test]
fn decoded_images_survive_source_envelope_and_raw_model_release() {
    let (base, _, large) = plans();
    let expected = ImagePlan::decode(&large).unwrap().next_fingerprint();
    let encoded = EnvelopePool::new(EnvelopeLimits::new(1, large.len() as u64).unwrap());
    let source = encoded.copy_encoded(&large).unwrap();
    let decoded = pool(1, charge(&large));
    let owner = source.decode_in(&decoded).unwrap();
    assert_eq!(encoded.usage().unwrap().buffers, 1);
    assert_eq!(decoded.usage().unwrap().plans, 1);
    drop(source);
    assert_eq!(encoded.usage().unwrap(), EnvelopeUsage::default());
    let output = owner.plan().replay(&base).unwrap();
    drop(base);
    drop(large);
    assert_eq!(output.fingerprint(), expected);
    assert_eq!(owner.plan().next_fingerprint(), expected);
    drop(output);
    drop(owner);
    assert_eq!(decoded.usage().unwrap(), DecodedPlanUsage::default());
}

#[test]
fn decoded_structure_does_not_authorize_a_foreign_or_stale_base() {
    let (base, small, _) = plans();
    let pool = pool(1, charge(&small));
    let owner = pool.decode(&small).unwrap();
    assert!(owner.plan().replay(&Model::new([9; 16]).unwrap()).is_err());
    let mut newer = base.clone();
    let mut staged = newer.begin().unwrap();
    staged.rebuild_index("items").unwrap();
    newer.publish(staged.prepare().unwrap()).unwrap();
    assert!(owner.plan().replay(&newer).is_err());
    assert_eq!(pool.usage().unwrap().plans, 1);
    verifies(&owner, &base, &small);
}

#[test]
fn actual_4096_separate_owners_fill_the_object_cap_and_release() {
    let (_, small, _) = plans();
    let bytes = charge(&small);
    let pool = pool(MAX_DECODED_PLANS, bytes * MAX_DECODED_PLANS as u64);
    let mut owners: Vec<_> = (0..MAX_DECODED_PLANS)
        .map(|_| pool.decode(&small).unwrap())
        .collect();
    assert_eq!(
        pool.usage().unwrap(),
        DecodedPlanUsage {
            plans: MAX_DECODED_PLANS,
            bytes: bytes * MAX_DECODED_PLANS as u64
        }
    );
    assert!(matches!(
        pool.decode(&small),
        Err(Error::DecodedAdmission(DecodedPlanLimit::Plans))
    ));
    drop(owners.pop());
    let replacement = pool.decode(&small).unwrap();
    drop(owners);
    assert_eq!(pool.usage().unwrap().plans, 1);
    drop(replacement);
    assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
}

fn race(byte_limited: bool) {
    let (base, small, large) = plans();
    let bytes = if byte_limited { &large } else { &small };
    let length = charge(bytes);
    let winners = if byte_limited { 3 } else { 2 };
    let pool = pool(
        if byte_limited { 8 } else { winners },
        length * winners as u64,
    );
    let start = Arc::new(Barrier::new(9));
    let retained = Arc::new(Barrier::new(9));
    let released = Arc::new(Barrier::new(9));
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let base = &base;
                let pool = pool.clone();
                let start = Arc::clone(&start);
                let retained = Arc::clone(&retained);
                let released = Arc::clone(&released);
                scope.spawn(move || {
                    start.wait();
                    let result = pool.decode(bytes);
                    let success = result.is_ok();
                    if let Ok(owner) = &result {
                        verifies(owner, base, bytes);
                    }
                    retained.wait();
                    released.wait();
                    drop(result);
                    success
                })
            })
            .collect();
        start.wait();
        retained.wait();
        assert_eq!(
            pool.usage().unwrap(),
            DecodedPlanUsage {
                plans: winners,
                bytes: length * winners as u64
            }
        );
        released.wait();
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.into_iter().filter(|ok| *ok).count(), winners);
    assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
}

#[test]
fn eight_threads_cannot_exceed_retained_object_slots() {
    race(false);
}

#[test]
fn eight_threads_cannot_overcommit_vector_payload_bytes() {
    race(true);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn independent_owner_and_handle_model_predicts_every_reservation(
        actions in prop::collection::vec((0u8..4, any::<u8>()), 0..100),
        plan_limit in 0usize..7, units in 0u64..5
    ) {
        let (base, small, large) = plans();
        let encoded = [&small, &large];
        let lengths = [charge(&small), charge(&large)];
        let byte_limit = lengths[1] * units;
        let pool = pool(plan_limit, byte_limit);
        let mut handles: Vec<(usize, usize, AdmittedPlan)> = Vec::new();
        let mut next_id = 0usize;
        for (action, number) in actions {
            let mut live = std::collections::BTreeMap::new();
            for (id, kind, _) in &handles { live.insert(*id, *kind); }
            let used: u64 = live.values().map(|kind| lengths[*kind]).sum();
            let kind = usize::from(number) % 2;
            if action == 0 {
                let expected = if live.len() == plan_limit { Err(DecodedPlanLimit::Plans) }
                    else if used + lengths[kind] > byte_limit { Err(DecodedPlanLimit::Bytes) } else { Ok(()) };
                match (pool.decode(encoded[kind]), expected) {
                    (Ok(owner), Ok(())) => { handles.push((next_id, kind, owner)); next_id += 1; },
                    (Err(Error::DecodedAdmission(actual)), Err(wanted)) => prop_assert_eq!(actual, wanted),
                    _ => prop_assert!(false, "decoded reservation disagrees with independent model"),
                }
            } else if !handles.is_empty() {
                let index = usize::from(number) % handles.len();
                match action {
                    1 => { let (id, kind, owner) = &handles[index]; handles.push((*id, *kind, owner.clone())); },
                    2 => { handles.remove(index); },
                    _ => verifies(&handles[index].2, &base, encoded[handles[index].1]),
                }
            }
            let mut live = std::collections::BTreeMap::new();
            for (id, kind, owner) in &handles { live.insert(*id, *kind); verifies(owner, &base, encoded[*kind]); }
            prop_assert_eq!(pool.usage().unwrap(), DecodedPlanUsage { plans: live.len(), bytes: live.values().map(|kind| lengths[*kind]).sum() });
        }
        drop(handles);
        prop_assert_eq!(pool.usage().unwrap(), DecodedPlanUsage::default());
    }
}

#[test]
fn vector_arithmetic_includes_addresses_roots_and_all_retirement_metadata() {
    let counts = PlanCounts::from_counts(2, 3, 4, 5, 6).unwrap();
    let expected = 5 * std::mem::size_of::<emilybase_commit_model::PageWrite>()
        + 4 * std::mem::size_of::<emilybase_commit_format::PageAddress>()
        + 5 * std::mem::size_of::<emilybase_commit_model::RootChange>()
        + 6 * std::mem::size_of::<emilybase_commit_model::RetiredRoot>();
    assert_eq!(counts.decoded_vector_bytes().unwrap(), expected as u64);
    assert!(counts.decoded_vector_bytes().unwrap() > counts.image_body_bytes());
}
