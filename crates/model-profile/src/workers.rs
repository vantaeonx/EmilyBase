//! Scoped diagnostic workers with a shared start and cancellation on spawn failure.
use crate::Error;
use emilybase_commit_model::{ImagePlan, Model};
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

struct Start {
    open: Mutex<bool>,
    changed: Condvar,
    cancelled: AtomicBool,
}

impl Start {
    fn new() -> Self {
        Self {
            open: Mutex::new(false),
            changed: Condvar::new(),
            cancelled: AtomicBool::new(false),
        }
    }
    fn wait(&self) -> Result<(), Error> {
        let mut open = self.open.lock().map_err(|_| Error::Worker("start lock"))?;
        while !*open {
            open = self
                .changed
                .wait(open)
                .map_err(|_| Error::Worker("start wait"))?;
        }
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::Worker("cancelled"));
        }
        Ok(())
    }
    fn release(&self, cancel: bool) {
        if cancel {
            self.cancelled.store(true, Ordering::Release);
        }
        // Cleanup must release already-started threads even after poison/failure.
        let mut open = self.open.lock().unwrap_or_else(|error| error.into_inner());
        *open = true;
        self.changed.notify_all();
    }
}

pub fn replay(plans: &[ImagePlan], bases: &[Model]) -> Result<Vec<Model>, Error> {
    if plans.is_empty() || plans.len() > 4 || plans.len() != bases.len() {
        return Err(Error::Worker("project count"));
    }
    let start = Start::new();
    let (ready, arrivals) = mpsc::channel();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(plans.len());
        for (plan, base) in plans.iter().zip(bases) {
            let sender = ready.clone();
            let gate = &start;
            let spawned = std::thread::Builder::new().spawn_scoped(scope, move || {
                // Report readiness before touching the gate; even a poisoned
                // gate cannot leave the coordinator waiting for this arrival.
                sender
                    .send(())
                    .map_err(|_| Error::Worker("ready channel"))?;
                gate.wait()?;
                Ok::<_, Error>(plan.replay(base)?)
            });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    start.release(true);
                    for handle in handles {
                        let _ = handle.join();
                    }
                    return Err(Error::Worker("spawn"));
                }
            }
        }
        drop(ready);
        for _ in &handles {
            if arrivals.recv_timeout(Duration::from_secs(30)).is_err() {
                start.release(true);
                for handle in handles {
                    let _ = handle.join();
                }
                return Err(Error::Worker("ready timeout"));
            }
        }
        start.release(false);
        let mut outputs = Vec::with_capacity(plans.len());
        let mut failure = None;
        for handle in handles {
            match handle.join() {
                Ok(Ok(model)) => outputs.push(model),
                Ok(Err(error)) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
                Err(_) => {
                    if failure.is_none() {
                        failure = Some(Error::Worker("panic"));
                    }
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(outputs),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emilybase_catalog::{Column, DataType, Schema};
    use emilybase_database::{Event, EventKind};

    fn fixtures(count: u8) -> (Vec<Model>, Vec<ImagePlan>) {
        let mut bases = Vec::new();
        let mut plans = Vec::new();
        for number in 1..=count {
            let base = Model::new([number; 16]).unwrap();
            let mut stage = base.begin().unwrap();
            stage
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
            stage.rebuild_index("items").unwrap();
            plans.push(stage.prepare().unwrap().image_plan().unwrap());
            bases.push(base);
        }
        (bases, plans)
    }

    #[test]
    fn cancelled_start_releases_all_real_waiters_without_running_work() {
        let gate = Start::new();
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let ready = sender.clone();
                    let gate = &gate;
                    scope.spawn(move || {
                        ready.send(()).unwrap();
                        gate.wait()
                    })
                })
                .collect();
            for _ in 0..4 {
                receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            gate.release(true);
            for handle in handles {
                assert!(matches!(
                    handle.join().unwrap(),
                    Err(Error::Worker("cancelled"))
                ));
            }
        });
    }

    #[test]
    fn poisoned_gate_cleanup_still_opens_the_start_and_fails_closed() {
        let gate = Start::new();
        std::thread::scope(|scope| {
            let result = scope
                .spawn(|| {
                    let _held = gate.open.lock().unwrap();
                    panic!("synthetic gate poison");
                })
                .join();
            assert!(result.is_err());
        });
        gate.release(true);
        assert!(*gate.open.lock().unwrap_err().into_inner());
        assert!(matches!(gate.wait(), Err(Error::Worker("start lock"))));
    }

    #[test]
    fn real_scoped_outputs_keep_order_exact_identity_and_all_bases_immutable() {
        let (bases, plans) = fixtures(4);
        let before: Vec<_> = bases.iter().map(Model::fingerprint).collect();
        let outputs = replay(&plans, &bases).unwrap();
        assert_eq!(outputs.len(), 4);
        for ((base, plan), output) in bases.iter().zip(&plans).zip(outputs) {
            assert_eq!(output.database_id(), base.database_id());
            assert_eq!(output.fingerprint(), plan.next_fingerprint());
            assert_eq!(output.view().table_id("items").unwrap(), 1);
            assert!(base.view().schemas().is_empty());
        }
        assert_eq!(
            before,
            bases.iter().map(Model::fingerprint).collect::<Vec<_>>()
        );
    }

    #[test]
    fn one_foreign_input_rejects_the_group_after_all_workers_are_joined() {
        let (mut bases, plans) = fixtures(4);
        bases[2] = bases[0].clone();
        let before: Vec<_> = bases.iter().map(Model::fingerprint).collect();
        assert!(matches!(
            replay(&plans, &bases),
            Err(Error::Model(emilybase_commit_model::Error::Conflict))
        ));
        assert_eq!(
            before,
            bases.iter().map(Model::fingerprint).collect::<Vec<_>>()
        );
    }

    #[test]
    fn mismatched_empty_and_excessive_groups_refuse_before_spawning() {
        let (bases, plans) = fixtures(5);
        for (plans, bases) in [
            (&plans[..0], &bases[..0]),
            (&plans[..2], &bases[..1]),
            (&plans[..], &bases[..]),
        ] {
            assert!(matches!(
                replay(plans, bases),
                Err(Error::Worker("project count"))
            ));
        }
    }
}
