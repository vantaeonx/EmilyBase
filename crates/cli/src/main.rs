use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use emilybase_storage::{Error, FORMAT_VERSION, PAGE_SIZE, Page, Pager, SlotId};

#[derive(Parser)]
#[command(
    version,
    about = "Experimental EmilyBase page storage; no transactions yet"
)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
            let mut page = db.read_page(page)?;
            page.update(slot, text.as_bytes())?;
            db.write_page(&page)?;
            println!("updated");
        }
        Command::Delete { path, page, slot } => {
            let mut db = Pager::open(path)?;
            let mut page = db.read_page(page)?;
            page.delete(slot)?;
            db.write_page(&page)?;
            println!("deleted");
        }
    }
    Ok(())
}
