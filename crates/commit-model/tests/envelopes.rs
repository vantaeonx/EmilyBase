use emilybase_catalog::{DataType, Key, Value};
use emilybase_commit_model::{
    AdmissionLimits, EnvelopeLimit, EnvelopeLimits, EnvelopePool, EnvelopeUsage, Error,
    IMAGE_PLAN_MAX_BYTES, ImagePlan, MAX_ENVELOPE_BUFFERS, MAX_ENVELOPE_BYTES, Model, ModelPool,
};
use emilybase_database::{Event, EventKind};
use proptest::prelude::*;
use std::sync::{Arc, Barrier};
mod support;
use support::{model, schema};

fn plans() -> (Model, ImagePlan, ImagePlan) {
    let base = model(&[("items", DataType::Integer)]);
    let mut staged = base.begin().unwrap();
    staged.rebuild_index("items").unwrap();
    let small = staged.prepare().unwrap().image_plan().unwrap();
    let mut staged = base.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(7), Value::Text("synthetic".into())]),
        })
        .unwrap();
    staged.rebuild_index("items").unwrap();
    let large = staged.prepare().unwrap().image_plan().unwrap();
    (base, small, large)
}
fn pool(buffers: usize, bytes: u64) -> EnvelopePool {
    EnvelopePool::new(EnvelopeLimits::new(buffers, bytes).unwrap())
}

#[test]
fn explicit_configuration_checks_hard_bounds_and_zero_disables_access() {
    assert!(EnvelopeLimits::new(MAX_ENVELOPE_BUFFERS, MAX_ENVELOPE_BYTES).is_ok());
    for (buffers, bytes) in [
        (MAX_ENVELOPE_BUFFERS + 1, 0),
        (usize::MAX, 0),
        (1, MAX_ENVELOPE_BYTES + 1),
        (1, u64::MAX),
    ] {
        assert!(matches!(
            EnvelopeLimits::new(buffers, bytes),
            Err(Error::EnvelopeConfiguration)
        ));
    }
    let (_, small, _) = plans();
    for (buffers, bytes, expected) in [
        (0, MAX_ENVELOPE_BYTES, EnvelopeLimit::Buffers),
        (1, 0, EnvelopeLimit::Bytes),
        (0, 0, EnvelopeLimit::Buffers),
    ] {
        let pool = pool(buffers, bytes);
        assert_eq!(pool.limits().buffers(), buffers);
        assert_eq!(pool.limits().bytes(), bytes);
        assert!(
            matches!(pool.encode(&small), Err(Error::EnvelopeAdmission(value)) if value == expected)
        );
        assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
    }
}

#[test]
fn exact_byte_limit_succeeds_and_one_less_refuses_without_partial_reservation() {
    let (base, small, _) = plans();
    let length = small.counts().unwrap().envelope_bytes().unwrap();
    assert_eq!(length, 424);
    let refused = pool(1, length - 1);
    assert!(matches!(
        refused.encode(&small),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Bytes))
    ));
    assert_eq!(refused.usage().unwrap(), EnvelopeUsage::default());
    let exact = pool(1, length);
    let envelope = exact.encode(&small).unwrap();
    assert_eq!(
        exact.usage().unwrap(),
        EnvelopeUsage {
            buffers: 1,
            bytes: length
        }
    );
    assert_eq!(
        ImagePlan::decode(envelope.as_bytes())
            .unwrap()
            .replay(&base)
            .unwrap()
            .fingerprint(),
        small.next_fingerprint()
    );
    assert!(matches!(
        envelope.try_clone(),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))
    ));
    assert_eq!(exact.usage().unwrap().bytes, length);
    drop(envelope);
    assert_eq!(exact.usage().unwrap(), EnvelopeUsage::default());
    assert!(exact.encode(&small).is_ok());
}

#[test]
fn mixed_size_buffers_charge_exact_lengths_and_reclaim_only_dropped_bytes() {
    let (_, small, large) = plans();
    let small_bytes = small.counts().unwrap().envelope_bytes().unwrap();
    let large_bytes = large.counts().unwrap().envelope_bytes().unwrap();
    assert!(large_bytes > small_bytes);
    let pool = pool(3, small_bytes + large_bytes);
    let first = pool.encode(&small).unwrap();
    let second = pool.encode(&large).unwrap();
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 2,
            bytes: small_bytes + large_bytes
        }
    );
    assert!(matches!(
        first.try_clone(),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Bytes))
    ));
    drop(first);
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 1,
            bytes: large_bytes
        }
    );
    let replacement = pool.encode(&small).unwrap();
    drop(second);
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 1,
            bytes: small_bytes
        }
    );
    let new_large = pool.copy_encoded(&large.encode().unwrap()).unwrap();
    assert_eq!(new_large.as_bytes().len() as u64, large_bytes);
    drop(new_large);
    drop(replacement);
    assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
}

#[test]
fn explicit_clones_are_independent_fully_charged_copies_in_the_same_pool() {
    let (_, small, _) = plans();
    let pool = pool(2, 848);
    let first = pool.encode(&small).unwrap();
    let second = first.try_clone().unwrap();
    assert_eq!(first.as_bytes(), second.as_bytes());
    assert_ne!(first.as_bytes().as_ptr(), second.as_bytes().as_ptr());
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 2,
            bytes: 848
        }
    );
    assert!(matches!(
        second.try_clone(),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))
    ));
    drop(first);
    assert_eq!(pool.usage().unwrap().bytes, 424);
    let third = second.try_clone().unwrap();
    drop(second);
    drop(third);
    assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
}

#[test]
fn pool_handle_clones_share_accounting_and_envelopes_keep_the_ledger_alive() {
    let (_, small, _) = plans();
    let original = pool(2, 848);
    let observer = original.clone();
    let envelope = original.encode(&small).unwrap();
    drop(original);
    let second = envelope.try_clone().unwrap();
    assert_eq!(observer.usage().unwrap().buffers, 2);
    drop(second);
    drop(envelope);
    assert_eq!(observer.usage().unwrap(), EnvelopeUsage::default());
    assert!(observer.encode(&small).is_ok());
    let standalone = pool(2, 848).encode(&small).unwrap();
    let copied = standalone.try_clone().unwrap();
    assert_eq!(standalone.as_bytes(), copied.as_bytes());
}

#[test]
fn malformed_copy_is_refused_without_charging_bytes_even_when_access_is_disabled() {
    let (_, small, _) = plans();
    let encoded = small.encode().unwrap();
    let pool = pool(2, MAX_ENVELOPE_BYTES);
    for end in [0, 191, 192, encoded.len() - 1] {
        assert!(pool.copy_encoded(&encoded[..end]).is_err());
        assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
    }
    let mut damaged = encoded.clone();
    damaged[200] ^= 1;
    assert!(matches!(
        pool.copy_encoded(&damaged),
        Err(Error::PlanChecksum)
    ));
    assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
    let disabled = super_disabled_pool();
    assert!(matches!(
        disabled.copy_encoded(&damaged),
        Err(Error::PlanChecksum)
    ));
    let valid = pool.copy_encoded(&encoded).unwrap();
    assert_eq!(valid.as_bytes(), encoded);
    assert_eq!(pool.usage().unwrap().bytes, encoded.len() as u64);
}
fn super_disabled_pool() -> EnvelopePool {
    pool(0, 0)
}

#[test]
fn copied_envelope_owns_its_bytes_and_preserves_source_after_capacity_refusal() {
    let (_, small, _) = plans();
    let mut source = small.encode().unwrap();
    let pool = pool(1, 424);
    let envelope = pool.copy_encoded(&source).unwrap();
    let original = source.clone();
    source.fill(0);
    assert_eq!(envelope.as_bytes(), original);
    assert!(matches!(
        pool.copy_encoded(&original),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))
    ));
    assert_eq!(envelope.as_bytes(), original);
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 1,
            bytes: 424
        }
    );
}

fn concurrent_admission(bytes_limit: u64, buffer_limit: usize, accepted: usize) {
    let (_, small, _) = plans();
    let encoded = small.encode().unwrap();
    let pool = pool(buffer_limit, bytes_limit);
    let start = Arc::new(Barrier::new(9));
    let admitted = Arc::new(Barrier::new(9));
    let release = Arc::new(Barrier::new(9));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for index in 0..8 {
            let pool = pool.clone();
            let start = Arc::clone(&start);
            let admitted = Arc::clone(&admitted);
            let release = Arc::clone(&release);
            let encoded = &encoded;
            let plan = &small;
            workers.push(scope.spawn(move || {
                start.wait();
                let result = if index % 2 == 0 {
                    pool.encode(plan)
                } else {
                    pool.copy_encoded(encoded)
                };
                admitted.wait();
                release.wait();
                result
            }));
        }
        start.wait();
        admitted.wait();
        assert_eq!(
            pool.usage().unwrap(),
            EnvelopeUsage {
                buffers: accepted,
                bytes: 424 * accepted as u64
            }
        );
        release.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            results.iter().filter(|result| result.is_ok()).count(),
            accepted
        );
        for envelope in results.iter().filter_map(|result| result.as_ref().ok()) {
            assert_eq!(envelope.as_bytes(), encoded);
        }
        drop(results);
    });
    assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
}

#[test]
fn eight_actual_workers_atomically_admit_two_buffers_before_release() {
    concurrent_admission(MAX_ENVELOPE_BYTES, 2, 2);
}
#[test]
fn eight_actual_workers_atomically_admit_three_payloads_below_the_fourth_byte_boundary() {
    concurrent_admission(424 * 4 - 1, 8, 3);
}

#[test]
fn all_4096_small_buffers_are_real_copies_and_last_drop_releases_every_reservation() {
    let (_, small, _) = plans();
    let pool = pool(MAX_ENVELOPE_BUFFERS, MAX_ENVELOPE_BYTES);
    let first = pool.encode(&small).unwrap();
    let mut retained = vec![first];
    for _ in 1..MAX_ENVELOPE_BUFFERS {
        retained.push(retained[0].try_clone().unwrap());
    }
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 4096,
            bytes: 4096 * 424
        }
    );
    assert!(matches!(
        retained[0].try_clone(),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))
    ));
    let survivor = retained.pop().unwrap();
    drop(retained);
    assert_eq!(
        pool.usage().unwrap(),
        EnvelopeUsage {
            buffers: 1,
            bytes: 424
        }
    );
    drop(survivor);
    assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
}

#[test]
fn admitted_preparation_encodes_without_exporting_raw_state_or_releasing_writer_early() {
    let models = ModelPool::new(AdmissionLimits::new(1, 3, 2, 1).unwrap());
    let mut project = models.create([7; 16]).unwrap();
    let old = project.read().unwrap();
    let mut staged = project.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(7), Value::Text("synthetic".into())]),
        })
        .unwrap();
    staged.rebuild_index("items").unwrap();
    let prepared = staged.prepare().unwrap();
    let disabled = pool(0, 0);
    assert!(matches!(
        prepared.encode_in(&disabled),
        Err(Error::EnvelopeAdmission(EnvelopeLimit::Buffers))
    ));
    assert_eq!(models.usage().unwrap().writers, 1);
    let buffers = pool(2, MAX_ENVELOPE_BYTES);
    let envelope = prepared.encode_in(&buffers).unwrap();
    let copied = envelope.try_clone().unwrap();
    assert_eq!(models.usage().unwrap().generations, 2);
    assert_eq!(models.usage().unwrap().writers, 1);
    project.publish(prepared).unwrap();
    assert_eq!(models.usage().unwrap().writers, 0);
    assert_eq!(models.usage().unwrap().generations, 2);
    assert_eq!(old.row_count(), 0);
    assert_eq!(
        project
            .read()
            .unwrap()
            .get("items", &Key::Integer(7))
            .unwrap()
            .unwrap()[1],
        Value::Text("synthetic".into())
    );
    drop(old);
    drop(project);
    assert_eq!(models.usage().unwrap().projects, 0);
    assert_eq!(models.usage().unwrap().generations, 0);
    assert_eq!(buffers.usage().unwrap().buffers, 2);
    drop(copied);
    drop(envelope);
    assert_eq!(buffers.usage().unwrap(), EnvelopeUsage::default());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn exact_live_byte_and_buffer_usage_matches_an_independent_admission_sequence(
        limit in 0usize..=8, byte_units in 0u64..=40,
        actions in prop::collection::vec((0u8..6, any::<u8>()), 1..80)
    ) {
        let (base, small, large) = plans();
        let plans = [&small, &large];
        let sources = [small.encode().unwrap(), large.encode().unwrap()];
        let bytes_limit = byte_units * 424;
        let pool = pool(limit, bytes_limit);
        let mut retained = Vec::<(usize, emilybase_commit_model::AdmittedEnvelope)>::new();
        for (action,index) in actions {
            if action == 3 {
                if !retained.is_empty() { retained.swap_remove(usize::from(index) % retained.len()); }
            } else if action == 4 {
                retained.clear();
            } else {
                let source = if action == 2 && !retained.is_empty() { retained[usize::from(index) % retained.len()].0 } else { usize::from(index) % 2 };
                let live_bytes: u64 = retained.iter().map(|(kind,_)| sources[*kind].len() as u64).sum();
                let expected = if retained.len() == limit { Some(EnvelopeLimit::Buffers) }
                    else if live_bytes + sources[source].len() as u64 > bytes_limit { Some(EnvelopeLimit::Bytes) } else { None };
                let result = match action {
                    1 => pool.copy_encoded(&sources[source]),
                    2 if !retained.is_empty() => retained[usize::from(index) % retained.len()].1.try_clone(),
                    5 => pool.clone().encode(plans[source]),
                    _ => pool.encode(plans[source]),
                };
                match expected {
                    Some(expected) => prop_assert!(matches!(result, Err(Error::EnvelopeAdmission(actual)) if actual == expected)),
                    None => retained.push((source,result.unwrap())),
                }
            }
            let expected_bytes: u64 = retained.iter().map(|(kind,_)| sources[*kind].len() as u64).sum();
            prop_assert_eq!(pool.usage().unwrap(), EnvelopeUsage { buffers: retained.len(), bytes: expected_bytes });
            for (kind,envelope) in &retained {
                prop_assert_eq!(envelope.as_bytes(), sources[*kind].as_slice());
                prop_assert_eq!(ImagePlan::decode(envelope.as_bytes()).unwrap().replay(&base).unwrap().fingerprint(), plans[*kind].next_fingerprint());
            }
        }
        drop(retained);
        prop_assert_eq!(pool.usage().unwrap(), EnvelopeUsage::default());
    }
}

#[test]
fn complete_plan_inspection_is_borrowed_and_reports_exact_envelope_components() {
    let (_, _, large) = plans();
    let encoded = large.encode().unwrap();
    assert_eq!(
        ImagePlan::inspect_encoded(&encoded).unwrap(),
        large.counts().unwrap()
    );
    let limit = pool(1, encoded.len() as u64);
    let envelope = limit.copy_encoded(&encoded).unwrap();
    assert_eq!(
        ImagePlan::inspect_encoded(envelope.as_bytes())
            .unwrap()
            .envelope_bytes()
            .unwrap(),
        encoded.len() as u64
    );
    assert!(encoded.len() < IMAGE_PLAN_MAX_BYTES);
}
