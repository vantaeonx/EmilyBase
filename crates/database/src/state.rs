use std::collections::BTreeMap;
use std::sync::Arc;

use emilybase_catalog::{Key, Row, Schema};

use crate::{Error, Event, EventKind, MAX_EVENTS, MAX_ROWS, MAX_TABLES, Result};

#[derive(Clone)]
pub(crate) struct Table {
    pub schema: Schema,
    pub rows: BTreeMap<Key, Row>,
}

#[derive(Clone)]
pub(crate) struct State {
    pub tables: BTreeMap<u64, Arc<Table>>,
    pub next_id: u64,
    pub row_count: usize,
    pub event_count: usize,
}

impl State {
    pub fn new() -> Self {
        Self {
            tables: BTreeMap::new(),
            next_id: 1,
            row_count: 0,
            event_count: 1,
        }
    }

    pub fn table_id(&self, name: &str) -> Result<u64> {
        self.tables
            .iter()
            .find(|(_, table)| table.schema.name == name)
            .map(|(id, _)| *id)
            .ok_or(Error::NoTable)
    }

    pub fn table(&self, name: &str) -> Result<&Table> {
        self.tables
            .get(&self.table_id(name)?)
            .map(Arc::as_ref)
            .ok_or(Error::NoTable)
    }

    /// The same constraints protect live operations and persisted event replay.
    pub fn validate(&self, event: &Event) -> Result<()> {
        if self.event_count >= MAX_EVENTS {
            return Err(Error::Limit("events"));
        }
        match &event.kind {
            EventKind::Root => return Err(Error::Event("repeated root marker")),
            EventKind::Create(schema) => {
                schema.validate()?;
                if self.tables.len() >= MAX_TABLES {
                    return Err(Error::Limit("tables"));
                }
                if self
                    .tables
                    .values()
                    .any(|table| table.schema.name == schema.name)
                {
                    return Err(Error::TableExists);
                }
                if event.table_id != self.next_id || self.next_id == u64::MAX {
                    return Err(Error::Event("nonsequential table ID"));
                }
            }
            kind => {
                let table = self.tables.get(&event.table_id).ok_or(Error::NoTable)?;
                match kind {
                    EventKind::Insert(row) => {
                        let key = table.schema.key(row)?;
                        if table.rows.contains_key(&key) {
                            return Err(Error::DuplicateKey);
                        }
                        if self.row_count >= MAX_ROWS {
                            return Err(Error::Limit("live rows"));
                        }
                    }
                    EventKind::Replace(row) => {
                        let key = table.schema.key(row)?;
                        if !table.rows.contains_key(&key) {
                            return Err(Error::NoRow);
                        }
                    }
                    EventKind::Delete(key) => {
                        table.schema.validate_key(key)?;
                        if !table.rows.contains_key(key) {
                            return Err(Error::NoRow);
                        }
                    }
                    EventKind::Drop => (),
                    _ => return Err(Error::Event("unexpected kind")),
                }
            }
        }
        Ok(())
    }

    pub fn apply(&mut self, event: Event) -> Result<()> {
        self.validate(&event)?;
        match event.kind {
            EventKind::Create(schema) => {
                self.tables.insert(
                    event.table_id,
                    Arc::new(Table {
                        schema,
                        rows: BTreeMap::new(),
                    }),
                );
                self.next_id += 1;
            }
            EventKind::Drop => {
                let table = self.tables.remove(&event.table_id).ok_or(Error::NoTable)?;
                self.row_count -= table.rows.len();
            }
            EventKind::Insert(row) | EventKind::Replace(row) => {
                let table = self.tables.get_mut(&event.table_id).ok_or(Error::NoTable)?;
                let key = table.schema.key(&row)?;
                if Arc::make_mut(table).rows.insert(key, row).is_none() {
                    self.row_count += 1;
                }
            }
            EventKind::Delete(key) => {
                let table = self.tables.get_mut(&event.table_id).ok_or(Error::NoTable)?;
                Arc::make_mut(table).rows.remove(&key);
                self.row_count -= 1;
            }
            EventKind::Root => return Err(Error::Event("repeated root marker")),
        }
        self.event_count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emilybase_catalog::{Column, DataType, Value};

    fn schema(name: &str) -> Schema {
        Schema {
            name: name.into(),
            columns: vec![Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            }],
            primary_key: 0,
        }
    }

    #[test]
    fn global_row_limit_allows_updates_and_reclaims_deleted_capacity() {
        let mut state = State::new();
        state
            .apply(Event {
                table_id: 1,
                kind: EventKind::Create(schema("items")),
            })
            .unwrap();
        for key in 0..MAX_ROWS {
            state
                .apply(Event {
                    table_id: 1,
                    kind: EventKind::Insert(vec![Value::Integer(key as i64)]),
                })
                .unwrap();
        }
        let insert = Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(MAX_ROWS as i64)]),
        };
        assert!(matches!(
            state.validate(&insert),
            Err(Error::Limit("live rows"))
        ));
        state
            .apply(Event {
                table_id: 1,
                kind: EventKind::Replace(vec![Value::Integer(0)]),
            })
            .unwrap();
        state
            .apply(Event {
                table_id: 1,
                kind: EventKind::Delete(Key::Integer(0)),
            })
            .unwrap();
        state.apply(insert).unwrap();
        assert_eq!(state.row_count, MAX_ROWS);
        state
            .apply(Event {
                table_id: 1,
                kind: EventKind::Drop,
            })
            .unwrap();
        assert_eq!(state.row_count, 0);
    }

    #[test]
    fn table_and_event_limits_are_checked_before_mutation() {
        let mut state = State::new();
        for index in 0..MAX_TABLES {
            state
                .apply(Event {
                    table_id: index as u64 + 1,
                    kind: EventKind::Create(schema(&format!("t{index}"))),
                })
                .unwrap();
        }
        let extra = Event {
            table_id: state.next_id,
            kind: EventKind::Create(schema("extra")),
        };
        assert!(matches!(
            state.validate(&extra),
            Err(Error::Limit("tables"))
        ));
        state
            .apply(Event {
                table_id: 1,
                kind: EventKind::Drop,
            })
            .unwrap();
        state.apply(extra).unwrap();
        assert_eq!(state.tables.len(), MAX_TABLES);
        state.event_count = MAX_EVENTS;
        assert!(matches!(
            state.validate(&Event {
                table_id: 2,
                kind: EventKind::Drop
            }),
            Err(Error::Limit("events"))
        ));
        assert_eq!(state.tables.len(), MAX_TABLES);
    }
}
