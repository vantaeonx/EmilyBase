use crate::support;
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_commit_format::{IndexKeyType, PageAddress, Predecessor, RootBinding};
use emilybase_commit_model::{MAX_EVENTS, Model, Staged};
use emilybase_database::{Event, EventKind};
use emilybase_index::IndexSnapshot;
use emilybase_model_profile::{Config, Error, Kind, PhaseKind, Report};

fn index(base: &Model, staged: &mut Staged, kind: Kind) -> Result<(), Error> {
    let tree = staged.view()?.export_primary_tree("items")?;
    let info = staged.view()?.verify_primary_tree("items", &tree)?;
    let previous = base.selection(1);
    let revision = previous.map_or(1, |selection| selection.binding().revision() + 1);
    let predecessor = previous
        .map(|selection| {
            Predecessor::new(
                selection.binding().revision(),
                selection.binding().transaction(),
                selection.index_fingerprint(),
            )
        })
        .transpose()?;
    let binding = RootBinding::new(
        PageAddress::primary(base.database_id(), 1, tree.root_id())?,
        if kind == Kind::Integer {
            IndexKeyType::Integer
        } else {
            IndexKeyType::Text
        },
        revision,
        staged.transaction(),
        info.entries as u64,
        info.excluded_long_keys as u64,
        info.pages as u32,
        predecessor,
    )?;
    staged.index(binding, IndexSnapshot { revision, tree })?;
    Ok(())
}

fn build(config: Config, project: u8) -> Result<Model, Error> {
    let mut model = Model::new([project + 1; 16])?;
    let mut create = model.begin()?;
    create.apply(Event {
        table_id: 1,
        kind: EventKind::Create(Schema {
            name: "items".into(),
            primary_key: 0,
            columns: vec![
                Column {
                    name: "id".into(),
                    data_type: support::data_type(config.kind),
                    nullable: false,
                },
                Column {
                    name: "value".into(),
                    data_type: DataType::Text,
                    nullable: false,
                },
            ],
        }),
    })?;
    index(&model, &mut create, config.kind)?;
    model.publish(create.prepare()?)?;
    for start in (0..config.rows).step_by(MAX_EVENTS) {
        let mut staged = model.begin()?;
        for number in start..(usize::from(start) + MAX_EVENTS).min(usize::from(config.rows)) as u16
        {
            staged.apply(Event {
                table_id: 1,
                kind: EventKind::Insert(support::row(
                    config.kind,
                    number,
                    "v".repeat(config.value_bytes as usize),
                )),
            })?;
        }
        index(&model, &mut staged, config.kind)?;
        model.publish(staged.prepare()?)?;
    }
    Ok(model)
}

pub fn measure(config: Config) -> Result<Report, Error> {
    config.validate()?;
    let mut phases = Vec::with_capacity(8);
    let mut models = Vec::with_capacity(config.projects as usize);
    for project in 0..config.projects {
        models.push(build(config, project)?);
    }
    if support::heap().allocated_blocks == 0 {
        return Err(Error::Instrumentation);
    }
    support::sample(&mut phases, PhaseKind::Built);
    let held: Vec<_> = if config.retain_old {
        models.clone()
    } else {
        Vec::new()
    };
    let mut stages: Vec<_> = models.iter().map(Model::begin).collect::<Result<_, _>>()?;
    support::sample(&mut phases, PhaseKind::Staged);
    for (model, staged) in models.iter().zip(&mut stages) {
        staged.apply(Event {
            table_id: 1,
            kind: EventKind::Replace(support::row(
                config.kind,
                0,
                "n".repeat(config.value_bytes as usize),
            )),
        })?;
        index(model, staged, config.kind)?;
    }
    support::sample(&mut phases, PhaseKind::IndexesStaged);
    let prepared = stages
        .into_iter()
        .map(Staged::prepare)
        .collect::<Result<Vec<_>, _>>()?;
    support::sample(&mut phases, PhaseKind::Prepared);
    for (model, prepared) in models.iter_mut().zip(prepared) {
        model.publish(prepared)?;
    }
    support::sample(&mut phases, PhaseKind::Published);
    for model in &models {
        if model.view().row_count() != config.rows as usize
            || model
                .view()
                .get("items", &support::key(config.kind, 0))?
                .is_none_or(|row| row[1] != Value::Text("n".repeat(config.value_bytes as usize)))
        {
            return Err(Error::Verification);
        }
    }
    for old in &held {
        if old
            .view()
            .get("items", &support::key(config.kind, 0))?
            .is_none_or(|row| row[1] != Value::Text("v".repeat(config.value_bytes as usize)))
        {
            return Err(Error::Verification);
        }
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
        version: 1,
        config,
        phases,
        components_per_project,
        comparison: None,
    })
}
