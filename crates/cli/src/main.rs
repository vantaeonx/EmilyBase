use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use emilybase_catalog::{Key, Row, Schema};
use emilybase_database::{DATABASE_MARKER, Database};
use emilybase_storage::{Error, FORMAT_VERSION, PAGE_SIZE, Page, Pager, SlotId};
mod tables;
use tables::Tables;

#[derive(Parser)]
#[command(
    version,
    about = "Experimental EmilyBase database; synthetic data only"
)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Atomically create an initialized table database.
    DbInit {
        path: PathBuf,
        /// Create a managed directory with mandatory WAL and transaction support.
        #[arg(long)]
        durable: bool,
    },
    /// Execute a bounded JSON operation array as one transaction.
    Tx {
        path: PathBuf,
        operations: String,
        #[arg(long)]
        rollback: bool,
    },
    /// Materialize committed pages while retaining the full journal.
    Checkpoint { path: PathBuf },
    /// Explicitly compact repeated WAL images into a self-contained version-2 baseline.
    Compact { path: PathBuf },
    /// Create and verify a no-clobber backup of a managed database.
    Backup { path: PathBuf, target: PathBuf },
    /// Validate archive checksums and replay its complete table history.
    BackupVerify { path: PathBuf },
    /// Verify and restore into a new directory without replacing existing paths.
    Restore { backup: PathBuf, target: PathBuf },
    /// Create a table from an explicit JSON schema.
    TableCreate { path: PathBuf, schema: String },
    /// Print validated table schemas as JSON.
    TableList { path: PathBuf },
    /// Drop a table and its current rows.
    TableDrop { path: PathBuf, table: String },
    /// Insert a JSON array of typed values, checking primary-key uniqueness.
    RowInsert {
        path: PathBuf,
        table: String,
        row: String,
    },
    /// Read by a typed JSON key; print null when absent.
    RowGet {
        path: PathBuf,
        table: String,
        key: String,
    },
    /// Replace a row while preserving its primary key.
    RowUpdate {
        path: PathBuf,
        table: String,
        key: String,
        row: String,
    },
    /// Delete by a typed JSON primary key.
    RowDelete {
        path: PathBuf,
        table: String,
        key: String,
    },
    /// Print a bounded set of rows in primary-key order.
    RowScan {
        path: PathBuf,
        table: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Create an empty database without overwriting an existing file.
    Init { path: PathBuf },
    /// Validate the header and print basic file information.
    Info { path: PathBuf },
    /// Validate every data page and its checksum.
    Verify { path: PathBuf },
    /// Append a UTF-8 record to the last page or allocate another page.
    Append { path: PathBuf, text: String },
    /// Read a UTF-8 record by physical page and slot.
    Get {
        path: PathBuf,
        page: u64,
        slot: SlotId,
    },
    /// Replace one record; physical page writes are not crash-safe transactions.
    Replace {
        path: PathBuf,
        page: u64,
        slot: SlotId,
        text: String,
    },
    /// Delete a record; its slot may be reused later.
    Delete {
        path: PathBuf,
        page: u64,
        slot: SlotId,
    },
}

fn main() -> ExitCode {
    match run(Arguments::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Error variants never embed record contents.
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::DbInit { path, durable } => {
            if durable {
                emilybase_transactions::Database::create(path)?;
                println!("created experimental managed database with WAL");
            } else {
                Database::create(path)?;
                println!("created experimental legacy table database without WAL");
            }
        }
        Command::Tx {
            path,
            operations,
            rollback,
        } => {
            tables::batch(&path, parse_json(&operations)?, rollback)?;
        }
        Command::Checkpoint { path } => {
            emilybase_transactions::Database::open(path)?.checkpoint()?;
            println!("checkpoint synced; journal retained");
        }
        Command::Compact { path } => {
            let report = emilybase_transactions::Database::open(path)?.compact()?;
            println!(
                "compacted pages={} transaction={} wal_bytes={}->{}",
                report.pages,
                report.transaction,
                report.previous_wal_bytes,
                report.compacted_wal_bytes
            );
        }
        Command::Backup { path, target } => {
            let mut database = emilybase_transactions::Database::open(path)?;
            print_backup(emilybase_backup::create(&mut database, target)?);
        }
        Command::BackupVerify { path } => print_backup(emilybase_backup::inspect(path)?),
        Command::Restore { backup, target } => {
            print_backup(emilybase_backup::restore(backup, target)?)
        }
        Command::TableCreate { path, schema } => {
            let schema: Schema = parse_json(&schema)?;
            let mut db = Tables::open(path)?;
            println!("table_id={}", db.create_table(schema)?);
        }
        Command::TableList { path } => {
            let db = Tables::open(path)?;
            println!("{}", serde_json::to_string(&db.schemas()?)?);
        }
        Command::TableDrop { path, table } => {
            Tables::open(path)?.drop_table(&table)?;
            println!("dropped");
        }
        Command::RowInsert { path, table, row } => {
            let row: Row = parse_json(&row)?;
            let key = Tables::open(path)?.insert(&table, row)?;
            println!("{}", serde_json::to_string(&key)?);
        }
        Command::RowGet { path, table, key } => {
            let key: Key = parse_json(&key)?;
            let db = Tables::open(path)?;
            println!("{}", serde_json::to_string(&db.get(&table, &key)?)?);
        }
        Command::RowUpdate {
            path,
            table,
            key,
            row,
        } => {
            let key: Key = parse_json(&key)?;
            let row: Row = parse_json(&row)?;
            Tables::open(path)?.update(&table, &key, row)?;
            println!("updated");
        }
        Command::RowDelete { path, table, key } => {
            let key: Key = parse_json(&key)?;
            Tables::open(path)?.delete(&table, &key)?;
            println!("deleted");
        }
        Command::RowScan { path, table, limit } => {
            let db = Tables::open(path)?;
            println!("{}", serde_json::to_string(&db.scan(&table, limit)?)?);
        }
        Command::Init { path } => {
            Pager::create(path)?;
            println!("created experimental database");
        }
        Command::Info { path } => {
            let db = Pager::open(path)?;
            println!(
                "format={FORMAT_VERSION} page_size={PAGE_SIZE} data_pages={}",
                db.page_count()
            );
        }
        Command::Verify { path } => {
            let mut db = Pager::open(path)?;
            db.verify()?;
            println!("verified {} data pages", db.page_count());
        }
        Command::Append { path, text } => {
            let mut db = Pager::open(path)?;
            ensure_raw(&mut db)?;
            let mut page = if db.page_count() == 0 {
                Page::new(1)?
            } else {
                db.read_page(db.page_count())?
            };
            let slot = match page.insert(text.as_bytes()) {
                Ok(slot) => slot,
                Err(Error::PageFull) => {
                    page = Page::new(db.page_count() + 1)?;
                    page.insert(text.as_bytes())?
                }
                Err(error) => return Err(error.into()),
            };
            db.write_page(&page)?;
            println!("page={} slot={slot}", page.id());
        }
        Command::Get { path, page, slot } => {
            let mut db = Pager::open(path)?;
            let page = db.read_page(page)?;
            println!("{}", std::str::from_utf8(page.get(slot)?)?);
        }
        Command::Replace {
            path,
            page,
            slot,
            text,
        } => {
            let mut db = Pager::open(path)?;
            ensure_raw(&mut db)?;
            let mut page = db.read_page(page)?;
            page.update(slot, text.as_bytes())?;
            db.write_page(&page)?;
            println!("updated");
        }
        Command::Delete { path, page, slot } => {
            let mut db = Pager::open(path)?;
            ensure_raw(&mut db)?;
            let mut page = db.read_page(page)?;
            page.delete(slot)?;
            db.write_page(&page)?;
            println!("deleted");
        }
    }
    Ok(())
}

fn print_backup(report: emilybase_backup::Report) {
    println!(
        "verified tables={} rows={} transaction={} wal_bytes={}",
        report.tables, report.rows, report.last_transaction, report.wal_bytes
    );
}

fn parse_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Box<dyn std::error::Error>> {
    if text.len() > 16384 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "JSON input exceeds 16384 bytes",
        )
        .into());
    }
    serde_json::from_str(text).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid typed JSON input").into()
    })
}

fn ensure_raw(pager: &mut Pager) -> Result<(), Box<dyn std::error::Error>> {
    if pager.page_count() > 0 && pager.read_page(1)?.get(0).ok() == Some(DATABASE_MARKER.as_slice())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "raw mutation is disabled for table databases; use table/row commands",
        )
        .into());
    }
    Ok(())
}
