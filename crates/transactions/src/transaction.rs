use emilybase_catalog::{Key, Row, Schema};
use emilybase_database::{Event, EventKind, Snapshot};

use crate::{Database, Error, MAX_TRANSACTION_EVENTS, Result};

/// Exclusive staged transaction. Any failed write aborts the entire transaction.
pub struct Transaction<'a> {
    pub(crate) database: &'a mut Database,
    pub(crate) staged: Snapshot,
    pub(crate) aborted: bool,
    pub(crate) events: usize,
}

impl Transaction<'_> {
    pub fn create_table(&mut self, schema: Schema) -> Result<u64> {
        let table_id = self.staged.next_table_id();
        self.write(|_| {
            Ok(Event {
                table_id,
                kind: EventKind::Create(schema),
            })
        })?;
        Ok(table_id)
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        self.write(|snapshot| {
            Ok(Event {
                table_id: snapshot.table_id(name)?,
                kind: EventKind::Drop,
            })
        })
    }

    pub fn insert(&mut self, name: &str, row: Row) -> Result<Key> {
        let mut key = None;
        self.write(|snapshot| {
            key = Some(snapshot.schema(name)?.key(&row)?);
            Ok(Event {
                table_id: snapshot.table_id(name)?,
                kind: EventKind::Insert(row),
            })
        })?;
        key.ok_or(Error::History("validated insert has no key"))
    }

    pub fn update(&mut self, name: &str, key: &Key, row: Row) -> Result<()> {
        self.write(|snapshot| {
            let schema = snapshot.schema(name)?;
            schema.validate_key(key)?;
            if schema.key(&row)? != *key {
                return Err(emilybase_database::Error::PrimaryKeyChange.into());
            }
            Ok(Event {
                table_id: snapshot.table_id(name)?,
                kind: EventKind::Replace(row),
            })
        })
    }

    pub fn delete(&mut self, name: &str, key: &Key) -> Result<()> {
        self.write(|snapshot| {
            Ok(Event {
                table_id: snapshot.table_id(name)?,
                kind: EventKind::Delete(key.clone()),
            })
        })
    }

    pub fn view(&self) -> Result<&Snapshot> {
        self.ready()?;
        Ok(&self.staged)
    }

    pub fn commit(self) -> Result<u64> {
        self.ready()?;
        if self.events == 0 {
            return Ok(self.database.last_transaction());
        }
        let old_count = self.database.snapshot.page_count();
        let old_last = self
            .database
            .snapshot
            .pages()
            .last()
            .ok_or(Error::History("missing old page"))?;
        let staged_last = self
            .staged
            .pages()
            .nth(old_count - 1)
            .ok_or(Error::History("missing staged page"))?;
        let start = if staged_last == old_last {
            old_count
        } else {
            old_count - 1
        };
        let changed = self.staged.pages().skip(start).cloned().collect::<Vec<_>>();
        let transaction = match self.database.wal.append(&changed) {
            Ok(transaction) => transaction,
            Err(error) => {
                if matches!(
                    error,
                    emilybase_wal::Error::Io(_)
                        | emilybase_wal::Error::Poisoned
                        | emilybase_wal::Error::OutcomeUnknown { .. }
                ) {
                    self.database.poisoned = true;
                }
                return Err(error.into());
            }
        };
        self.database.snapshot = self.staged;
        Ok(transaction)
    }

    pub fn rollback(self) {
        // Staged events have never reached WAL or the committed snapshot.
    }

    fn ready(&self) -> Result<()> {
        self.database.ready()?;
        if self.aborted {
            Err(Error::Aborted)
        } else {
            Ok(())
        }
    }

    fn write(&mut self, make_event: impl FnOnce(&Snapshot) -> Result<Event>) -> Result<()> {
        self.ready()?;
        let result = (|| {
            if self.events >= MAX_TRANSACTION_EVENTS {
                return Err(Error::Limit);
            }
            let event = make_event(&self.staged)?;
            self.staged.apply(event)?;
            self.events += 1;
            Ok(())
        })();
        if result.is_err() {
            self.aborted = true;
        }
        result
    }
}
