//! Observe owned plans and independent memory replay before publication.
use crate::{state, support};
use emilybase_catalog::Value;
use emilybase_commit_model::{Model, Staged};
use emilybase_database::{Event, EventKind};
use emilybase_model_profile::{Config, Error, ImageComponents, Mode, PhaseKind, Report};

fn verify(model: &Model, config: Config, prefix: &str) -> Result<(), Error> {
    if model.view().row_count() != usize::from(config.rows) {
        return Err(Error::Verification);
    }
    for number in [0, config.rows - 1] {
        let key = support::key(config.kind, number);
        let row = model
            .view()
            .get("items", &key)?
            .ok_or(Error::Verification)?;
        let expected = if number == 0 { prefix } else { "v" };
        if row[1] != Value::Text(expected.repeat(usize::from(config.value_bytes))) {
            return Err(Error::Verification);
        }
        let location = model
            .view()
            .row_location("items", &key)?
            .ok_or(Error::Verification)?;
        if model.view().resolve_row_location("items", &key, location)? != row {
            return Err(Error::Verification);
        }
    }
    Ok(())
}

pub fn measure(config: Config) -> Result<Report, Error> {
    config.validate()?;
    if !matches!(config.mode, Mode::Replay | Mode::IndexReplay) {
        return Err(Error::Verification);
    }
    let mut phases = Vec::with_capacity(11);
    let mut models = (0..config.projects)
        .map(|project| state::build(config, project))
        .collect::<Result<Vec<_>, _>>()?;
    if support::heap().allocated_blocks == 0 {
        return Err(Error::Instrumentation);
    }
    support::sample(&mut phases, PhaseKind::Built);
    let held = if config.retain_old {
        models.clone()
    } else {
        Vec::new()
    };
    let mut stages = models
        .iter()
        .map(Model::begin)
        .collect::<Result<Vec<_>, _>>()?;
    support::sample(&mut phases, PhaseKind::Staged);
    for (base, stage) in models.iter().zip(&mut stages) {
        if config.mode == Mode::Replay {
            stage.apply(Event {
                table_id: 1,
                kind: EventKind::Replace(support::row(
                    config.kind,
                    0,
                    "n".repeat(usize::from(config.value_bytes)),
                )),
            })?;
        }
        state::index(base, stage, config.kind)?;
    }
    support::sample(&mut phases, PhaseKind::IndexesStaged);
    let prepared = stages
        .into_iter()
        .map(Staged::prepare)
        .collect::<Result<Vec<_>, _>>()?;
    support::sample(&mut phases, PhaseKind::Prepared);
    let plans = prepared
        .iter()
        .map(|value| value.image_plan())
        .collect::<Result<Vec<_>, _>>()?;
    let images_per_project = plans
        .iter()
        .map(|plan| {
            let counts = plan.counts()?;
            Ok(ImageComponents {
                history_pages: counts.history_pages() as u64,
                primary_pages: counts.primary_pages() as u64,
                retired_pages: counts.retired_pages() as u64,
                changed_roots: counts.changed_roots() as u64,
                retired_tables: counts.retired_tables() as u64,
                image_body_bytes: counts.image_body_bytes(),
            })
        })
        .collect::<Result<Vec<_>, emilybase_commit_model::Error>>()?;
    support::sample(&mut phases, PhaseKind::PlansBuilt);
    let replayed = if config.parallel {
        emilybase_model_profile::replay_parallel(&plans, &models)?
    } else {
        plans
            .iter()
            .zip(&models)
            .map(|(plan, base)| plan.replay(base))
            .collect::<Result<Vec<_>, _>>()?
    };
    support::sample(&mut phases, PhaseKind::Replayed);
    let prefix = if config.mode == Mode::Replay {
        "n"
    } else {
        "v"
    };
    for ((plan, base), replay) in plans.iter().zip(&models).zip(&replayed) {
        if base.fingerprint() != plan.base_fingerprint()
            || replay.fingerprint() != plan.next_fingerprint()
        {
            return Err(Error::Verification);
        }
        verify(base, config, "v")?;
        verify(replay, config, prefix)?;
    }
    drop(replayed);
    support::sample(&mut phases, PhaseKind::ReplayReleased);
    drop(plans);
    support::sample(&mut phases, PhaseKind::PlansReleased);
    for (base, prepared) in models.iter_mut().zip(prepared) {
        base.publish(prepared)?;
        verify(base, config, prefix)?;
    }
    support::sample(&mut phases, PhaseKind::Published);
    for old in &held {
        verify(old, config, "v")?;
    }
    drop(held);
    support::sample(&mut phases, PhaseKind::OldViewsReleased);
    let components_per_project = models
        .iter()
        .map(support::components)
        .collect::<Result<_, _>>()?;
    drop(models);
    support::sample(&mut phases, PhaseKind::Released);
    Ok(Report {
        version: if config.parallel { 3 } else { 2 },
        config,
        phases,
        components_per_project,
        comparison: None,
        images_per_project: Some(images_per_project),
    })
}
