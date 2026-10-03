use std::path::Path;

use emilybase_catalog::{Key, Row, Schema};
use emilybase_database::Database as Legacy;
use emilybase_transactions::{Database as Managed, Transaction};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub enum Tables {
    Legacy(Legacy),
    Managed(Managed),
}

impl Tables {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if std::fs::metadata(path)?.is_dir() {
            Ok(Self::Managed(Managed::open(path)?))
        } else {
            Ok(Self::Legacy(Legacy::open(path)?))
        }
    }

    pub fn create_table(&mut self, schema: Schema) -> Result<u64> {
        let legacy_schema = schema.clone();
        self.write(
            |db| db.create_table(legacy_schema),
            |tx| tx.create_table(schema),
        )
    }

    pub fn drop_table(&mut self, name: &str) -> Result<()> {
        self.write(|db| db.drop_table(name), |tx| tx.drop_table(name))
    }

    pub fn insert(&mut self, name: &str, row: Row) -> Result<Key> {
        let legacy_row = row.clone();
        self.write(|db| db.insert(name, legacy_row), |tx| tx.insert(name, row))
    }

    pub fn update(&mut self, name: &str, key: &Key, row: Row) -> Result<()> {
        let legacy_row = row.clone();
        self.write(
            |db| db.update(name, key, legacy_row),
            |tx| tx.update(name, key, row),
        )
    }

    pub fn delete(&mut self, name: &str, key: &Key) -> Result<()> {
        self.write(|db| db.delete(name, key), |tx| tx.delete(name, key))
    }

    pub fn get(&self, name: &str, key: &Key) -> Result<Option<&Row>> {
        match self {
            Self::Legacy(db) => Ok(db.get(name, key)?),
            Self::Managed(db) => Ok(db.view()?.get(name, key)?),
        }
    }

    pub fn scan(&self, name: &str, limit: usize) -> Result<Vec<Row>> {
        match self {
            Self::Legacy(db) => Ok(db.scan(name, limit)?),
            Self::Managed(db) => Ok(db.view()?.scan(name, limit)?),
        }
    }

    pub fn schemas(&self) -> Result<Vec<Schema>> {
        match self {
            Self::Legacy(db) => Ok(db.schemas()?),
            Self::Managed(db) => Ok(db.view()?.schemas()),
        }
    }

    fn write<T>(
        &mut self,
        legacy: impl FnOnce(&mut Legacy) -> emilybase_database::Result<T>,
        managed: impl FnOnce(&mut Transaction<'_>) -> emilybase_transactions::Result<T>,
    ) -> Result<T> {
        match self {
            Self::Legacy(db) => Ok(legacy(db)?),
            Self::Managed(db) => {
                let mut tx = db.begin()?;
                let result = managed(&mut tx)?;
                tx.commit()?;
                Ok(result)
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    CreateTable { schema: Schema },
    DropTable { table: String },
    Insert { table: String, row: Row },
    Update { table: String, key: Key, row: Row },
    Delete { table: String, key: Key },
}

pub fn batch(path: &Path, operations: Vec<Operation>, rollback: bool) -> Result<()> {
    if operations.len() > emilybase_transactions::MAX_TRANSACTION_EVENTS {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "too many transaction operations",
        )
        .into());
    }
    let mut db = Managed::open(path)?;
    let mut tx = db.begin()?;
    for operation in operations {
        match operation {
            Operation::CreateTable { schema } => {
                tx.create_table(schema)?;
            }
            Operation::DropTable { table } => tx.drop_table(&table)?,
            Operation::Insert { table, row } => {
                tx.insert(&table, row)?;
            }
            Operation::Update { table, key, row } => tx.update(&table, &key, row)?,
            Operation::Delete { table, key } => tx.delete(&table, &key)?,
        }
    }
    if rollback {
        tx.rollback();
        println!("rolled back");
    } else {
        println!("transaction={}", tx.commit()?);
    }
    Ok(())
}
