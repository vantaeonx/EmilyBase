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
    /// Inspect count-only optional-cache startup results after authoritative WAL recovery.
    PrimaryIndexCacheStatus { path: PathBuf },
    /// Save a private optional primary-tree image in the managed directory.
    PrimaryIndexSave { path: PathBuf, table: String },
    /// Explicitly verify and load the optional image; absence prints null.
    PrimaryIndexLoad { path: PathBuf, table: String },
    /// Inspect the derived managed primary-key B+ tree without persisting index pages.
    PrimaryIndexInfo { path: PathBuf, table: String },
    /// Inspect an experimental row-image location bound to a managed database identity.
    RowLocation {
        path: PathBuf,
        table: String,
        key: String,
    },
    /// Resolve only a current matching row-image location; never authorize through a locator.
    RowResolve {
        path: PathBuf,
        table: String,
        key: String,
        location: String,
    },
    /// Offline private backup of every isolated project, including current key digests.
    ProjectsBackup { path: PathBuf, target: PathBuf },
    /// Verify an explicit registry/private bundle; print counts without private contents.
    AccountBundleVerify { path: PathBuf },
    /// Capture the exact offline root manifest roster into a private bundle file.
    AccountRootBackup { path: PathBuf, target: PathBuf },
    /// Explicitly create one empty project/private store in a new root; no credentials printed.
    AccountRootInit {
        target: PathBuf,
        #[arg(long, allow_hyphen_values = true)]
        name: String,
        /// Required trusted initial Unix-time floor; never inferred or reset at startup.
        #[arg(long, allow_hyphen_values = true)]
        reset_at: String,
    },
    /// Verify an offline restored root; print aggregate counts only.
    AccountRootVerify { path: PathBuf },
    /// Restore a bundle into one new root and reset private sessions before selection.
    AccountBundleRestore {
        backup: PathBuf,
        target: PathBuf,
        /// Required trusted Unix time in seconds; no automatic clock or secret input.
        #[arg(long, allow_hyphen_values = true)]
        reset_at: String,
    },
    /// Replay and verify every project in a private registry archive.
    ProjectsBackupVerify { path: PathBuf },
    /// Restore all projects into a new registry without overwriting existing paths.
    ProjectsRestore { backup: PathBuf, target: PathBuf },
    /// Create a standalone stable B+ tree snapshot, separate from table/WAL storage.
    IndexCreate { path: PathBuf },
    /// Insert an opaque synthetic record pointer into a standalone index.
    IndexInsert {
        path: PathBuf,
        key: String,
        target_page: u64,
        target_slot: u16,
    },
    /// Read a standalone index pointer; does not dereference a table row.
    IndexGet { path: PathBuf, key: String },
    /// Read an ordered standalone key/pointer interval without resolving table rows.
    IndexRange {
        path: PathBuf,
        /// Inclusive tagged JSON key; absent means unbounded.
        #[arg(long)]
        lower: Option<String>,
        /// Exclusive tagged JSON key; absent means unbounded.
        #[arg(long)]
        upper: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        descending: bool,
    },
    /// Delete a standalone key and publish its next snapshot revision.
    IndexDelete { path: PathBuf, key: String },
    /// Validate the selected standalone snapshot and complete tree topology.
    IndexVerify { path: PathBuf },
    /// Execute the documented SQL subset as one managed transaction.
    Sql {
        path: PathBuf,
        sql: String,
        /// Separate JSON array of tagged typed values for $1, $2, ...
        #[arg(long, default_value = "[]")]
        parameters: String,
        /// Resolve one SELECT and print its plan without executing it.
        #[arg(long)]
        explain: bool,
    },
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
        Command::PrimaryIndexCacheStatus { path } => {
            let database = emilybase_transactions::Database::open(path)?;
            println!(
                "{}",
                serde_json::to_string(&database.primary_cache_startup()?)?
            );
        }
        Command::PrimaryIndexSave { path, table } => {
            let database = emilybase_transactions::Database::open(path)?;
            println!(
                "{}",
                serde_json::to_string(&database.save_primary_index_cache(&table)?)?
            );
        }
        Command::PrimaryIndexLoad { path, table } => {
            let mut database = emilybase_transactions::Database::open(path)?;
            println!(
                "{}",
                serde_json::to_string(&database.load_primary_index_cache(&table)?)?
            );
        }
        Command::PrimaryIndexInfo { path, table } => {
            let database = emilybase_transactions::Database::open(path)?;
            println!(
                "{}",
                serde_json::to_string(&database.view()?.primary_index_info(&table)?)?
            );
        }
        Command::RowLocation { path, table, key } => {
            let key: Key = parse_json(&key)?;
            let database = emilybase_transactions::Database::open(path)?;
            println!(
                "{}",
                serde_json::to_string(&database.row_location(&table, &key)?)?
            );
        }
        Command::RowResolve {
            path,
            table,
            key,
            location,
        } => {
            let key: Key = parse_json(&key)?;
            let location: emilybase_transactions::BoundRowLocation = parse_json(&location)?;
            let database = emilybase_transactions::Database::open(path)?;
            println!(
                "{}",
                serde_json::to_string(database.resolve_row_location(&table, &key, location)?)?
            );
        }
        Command::ProjectsBackup { path, target } => {
            let report = emilybase_server::ProjectStore::open_existing(path)?.backup(target)?;
            print_registry_backup(&report);
        }
        Command::AccountBundleVerify { path } => {
            let report = emilybase_server::inspect_account_bundle(path)?;
            print_bundle(&report);
        }
        Command::AccountRootBackup { path, target } => {
            let pool = emilybase_auth::password::PasswordPool::new(1)?;
            print_bundle(&emilybase_server::backup_account_bundle_root(
                path, target, pool,
            )?);
        }
        Command::AccountRootInit {
            target,
            name,
            reset_at,
        } => {
            let now = trusted_reset_time(&reset_at)?;
            let pool = emilybase_auth::password::PasswordPool::new(1)?;
            print_account_root(&emilybase_server::initialize_account_root(
                target, &name, pool, now,
            )?);
        }
        Command::AccountRootVerify { path } => {
            let pool = emilybase_auth::password::PasswordPool::new(1)?;
            print_account_root(&emilybase_server::inspect_account_bundle_root(path, pool)?);
        }
        Command::AccountBundleRestore {
            backup,
            target,
            reset_at,
        } => {
            let now = trusted_reset_time(&reset_at)?;
            let pool = emilybase_auth::password::PasswordPool::new(1)?;
            print_account_root(&emilybase_server::restore_account_bundle(
                backup, target, pool, now,
            )?);
        }
        Command::ProjectsBackupVerify { path } => {
            let report = emilybase_server::inspect_registry_backup(path)?;
            print_registry_backup(&report);
        }
        Command::ProjectsRestore { backup, target } => {
            let report = emilybase_server::restore_registry_backup(backup, target)?;
            print_registry_backup(&report);
        }
        Command::IndexCreate { path } => {
            emilybase_index::IndexStore::create(path, &emilybase_index::BPlusTree::new_stable())?;
            println!("created standalone experimental index; revision=1");
        }
        Command::IndexInsert {
            path,
            key,
            target_page,
            target_slot,
        } => {
            let key: Key = parse_json(&key)?;
            let mut store = emilybase_index::IndexStore::open(path)?;
            let mut tree = store.snapshot()?.tree.clone();
            tree.insert(
                key,
                emilybase_index::RecordPointer {
                    page_id: target_page,
                    slot_id: target_slot,
                },
            )?;
            println!("index revision={}", store.replace(&tree)?);
        }
        Command::IndexDelete { path, key } => {
            let key: Key = parse_json(&key)?;
            let mut store = emilybase_index::IndexStore::open(path)?;
            let mut tree = store.snapshot()?.tree.clone();
            tree.remove(&key)?;
            println!("index revision={}", store.replace(&tree)?);
        }
        Command::IndexGet { path, key } => {
            let key: Key = parse_json(&key)?;
            let store = emilybase_index::IndexStore::open(path)?;
            let result =
                store.snapshot()?.tree.get(&key)?.map(
                    |pointer| serde_json::json!({"page":pointer.page_id,"slot":pointer.slot_id}),
                );
            println!("{}", serde_json::to_string(&result)?);
        }
        Command::IndexRange {
            path,
            lower,
            upper,
            limit,
            descending,
        } => {
            if limit > emilybase_index::MAX_INDEX_ENTRIES {
                return Err(emilybase_index::Error::Limit.into());
            }
            let lower: Option<Key> = lower.as_deref().map(parse_json).transpose()?;
            let upper: Option<Key> = upper.as_deref().map(parse_json).transpose()?;
            let store = emilybase_index::IndexStore::open(path)?;
            let cursor = store
                .snapshot()?
                .tree
                .cursor(lower.as_ref(), upper.as_ref())?;
            let json = |entry: emilybase_index::Result<(&Key, emilybase_index::RecordPointer)>| {
                entry.map(|(key, pointer)| serde_json::json!({"key":key,"page":pointer.page_id,"slot":pointer.slot_id}))
            };
            let rows = if descending {
                cursor
                    .rev()
                    .take(limit)
                    .map(json)
                    .collect::<emilybase_index::Result<Vec<_>>>()?
            } else {
                cursor
                    .take(limit)
                    .map(json)
                    .collect::<emilybase_index::Result<Vec<_>>>()?
            };
            println!("{}", serde_json::to_string(&rows)?);
        }
        Command::IndexVerify { path } => {
            let store = emilybase_index::IndexStore::open(path)?;
            let snapshot = store.snapshot()?;
            println!(
                "verified index revision={} pages={} entries={}",
                snapshot.revision,
                snapshot.tree.page_count(),
                snapshot.tree.len()
            );
        }
        Command::Sql {
            path,
            sql,
            parameters,
            explain,
        } => {
            let parameters: Row = parse_json(&parameters)?;
            let mut database = emilybase_transactions::Database::open(path)?;
            if explain {
                let plan = emilybase_query::explain(database.view()?, &sql, &parameters)?;
                println!("{}", serde_json::to_string(&plan)?);
            } else {
                let report = emilybase_query::execute(&mut database, &sql, &parameters)?;
                println!("{}", serde_json::to_string(&report)?);
            }
        }
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

fn print_registry_backup(report: &emilybase_server::RegistryBackupReport) {
    println!(
        "verified registry projects={} tables={} rows={} archive_bytes={}",
        report.projects.len(),
        report.projects.iter().map(|p| p.tables).sum::<usize>(),
        report.projects.iter().map(|p| p.rows).sum::<usize>(),
        report.archive_bytes,
    );
}

fn trusted_reset_time(text: &str) -> Result<u64, emilybase_server::Error> {
    let invalid = || emilybase_server::Error::BundleRoot("trusted reset time");
    if text.is_empty() || text.len() > 19 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    text.parse::<u64>().map_err(|_| invalid()).and_then(|now| {
        if now > i64::MAX as u64 {
            Err(invalid())
        } else {
            Ok(now)
        }
    })
}
fn bundle_counts(
    registry: &emilybase_server::RegistryBackupReport,
    private: &[emilybase_server::BundledAccountReport],
) -> String {
    format!(
        "projects={} private_stores={} tables={} rows={} accounts={} session_families={}",
        registry.projects.len(),
        private.len(),
        registry.projects.iter().map(|p| p.tables).sum::<usize>(),
        registry.projects.iter().map(|p| p.rows).sum::<usize>(),
        private.iter().map(|p| p.inventory.accounts).sum::<usize>(),
        private
            .iter()
            .map(|p| p.inventory.session_families)
            .sum::<usize>()
    )
}
fn print_bundle(report: &emilybase_server::AccountBundleReport) {
    println!(
        "verified bundle {} archive_bytes={}",
        bundle_counts(&report.registry, &report.private_accounts),
        report.archive_bytes
    );
}
fn print_account_root(report: &emilybase_server::AccountBundleRootReport) {
    println!(
        "verified root {} reset_at={}",
        bundle_counts(&report.registry, &report.private_accounts),
        report.reset_at
    );
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
