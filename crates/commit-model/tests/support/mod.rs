use emilybase_catalog::{Column, DataType, Schema};
use emilybase_commit_format::{IndexKeyType, PageAddress, Predecessor, RootBinding};
use emilybase_commit_model::{Model, Staged};
use emilybase_database::{Event, EventKind};
use emilybase_index::{BPlusTree, IndexSnapshot};

pub fn schema(name: &str, data_type: DataType) -> Schema {
    Schema {
        name: name.into(),
        columns: vec![
            Column {
                name: "id".into(),
                data_type,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
        primary_key: 0,
    }
}

pub fn candidate(
    base: &Model,
    staged: &Staged,
    name: &str,
    tree: BPlusTree,
) -> (RootBinding, IndexSnapshot) {
    let view = staged.view().unwrap();
    let table = view.table_id(name).unwrap();
    let previous = base.selection(table);
    let revision = previous.map_or(1, |value| value.binding().revision() + 1);
    let predecessor = previous.map(|value| {
        Predecessor::new(
            value.binding().revision(),
            value.binding().transaction(),
            value.index().fingerprint().unwrap(),
        )
        .unwrap()
    });
    let info = view.verify_primary_tree(name, &tree).unwrap();
    let key_type = if view.schema(name).unwrap().columns[0].data_type == DataType::Integer {
        IndexKeyType::Integer
    } else {
        IndexKeyType::Text
    };
    let binding = RootBinding::new(
        PageAddress::primary(base.database_id(), table, tree.root_id()).unwrap(),
        key_type,
        revision,
        staged.transaction(),
        info.entries as u64,
        info.excluded_long_keys as u64,
        info.pages as u32,
        predecessor,
    )
    .unwrap();
    (binding, IndexSnapshot { revision, tree })
}

pub fn indexes(base: &Model, staged: &mut Staged, names: &[&str]) {
    for name in names {
        let tree = staged.view().unwrap().export_primary_tree(name).unwrap();
        let (binding, index) = candidate(base, staged, name, tree);
        staged.index(binding, index).unwrap();
    }
}

pub fn model(tables: &[(&str, DataType)]) -> Model {
    let mut model = Model::new([7; 16]).unwrap();
    let mut staged = model.begin().unwrap();
    for (name, data_type) in tables {
        staged
            .apply(Event {
                table_id: staged.view().unwrap().next_table_id(),
                kind: EventKind::Create(schema(name, *data_type)),
            })
            .unwrap();
    }
    let names = tables.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    indexes(&model, &mut staged, &names);
    model.publish(staged.prepare().unwrap()).unwrap();
    model
}
