use emilybase_catalog::{DataType, Key, Value};
use emilybase_model_profile::{Components, Error, Heap, Kind, Phase, PhaseKind};

pub fn heap() -> Heap {
    let value = dhat::HeapStats::get();
    Heap {
        current_bytes: value.curr_bytes as u64,
        current_blocks: value.curr_blocks as u64,
        peak_bytes: value.max_bytes as u64,
        peak_blocks: value.max_blocks as u64,
        allocated_bytes: value.total_bytes,
        allocated_blocks: value.total_blocks,
    }
}

pub fn sample(phases: &mut Vec<Phase>, phase: PhaseKind) {
    phases.push(Phase {
        phase,
        heap: heap(),
    });
}

pub fn key(kind: Kind, number: u16) -> Key {
    match kind {
        Kind::Integer => Key::Integer(i64::from(number)),
        Kind::ShortText | Kind::LongText => {
            let bytes = if kind == Kind::ShortText { 256 } else { 3072 };
            Key::Text(format!("key-{number:08}{}", "я".repeat((bytes - 12) / 2)))
        }
    }
}

pub fn data_type(kind: Kind) -> DataType {
    if kind == Kind::Integer {
        DataType::Integer
    } else {
        DataType::Text
    }
}

pub fn row(kind: Kind, number: u16, value: String) -> Vec<Value> {
    vec![key(kind, number).to_value(), Value::Text(value)]
}

pub fn components(model: &emilybase_commit_model::Model) -> Result<Components, Error> {
    let value = model.encoded_components()?;
    Ok(Components {
        history_pages: value.history_pages(),
        index_pages: value.index_pages(),
        root_bytes: value.root_bytes(),
        index_bytes: value.index_bytes(),
        total_bytes: value.total_bytes(),
    })
}
