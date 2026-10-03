use emilybase_database::{DATABASE_MARKER, Snapshot};
use emilybase_storage::Page;
use emilybase_wal::Recovery;

use crate::{Error, Result};

pub(crate) fn replay(recovery: Recovery) -> Result<Snapshot> {
    let mut pages: Vec<Page> = recovery.baseline.map_or_else(Vec::new, |batch| batch.pages);
    for batch in recovery.committed {
        if pages.is_empty() {
            if batch.transaction != 1 || batch.pages.len() != 1 {
                return Err(Error::History("initial transaction"));
            }
            let root = &batch.pages[0];
            if root.id() != 1 || root.slot_count() != 1 || root.get(0)? != DATABASE_MARKER {
                return Err(Error::History("initial root image"));
            }
        } else {
            let first = batch.pages.first().ok_or(Error::History("empty batch"))?;
            let last = pages.len() as u64;
            if first.id() != last && first.id() != last + 1 {
                return Err(Error::History("redo changes an older page or leaves a gap"));
            }
            if first.id() == last {
                let old = pages.last().ok_or(Error::History("missing old page"))?;
                if first.slot_count() <= old.slot_count() {
                    return Err(Error::History("redo does not append history"));
                }
                for slot in 0..old.slot_count() {
                    if old.get(slot as u16)? != first.get(slot as u16)? {
                        return Err(Error::History("redo rewrites committed history"));
                    }
                }
            }
        }
        let first_id = batch.pages[0].id();
        for (index, page) in batch.pages.into_iter().enumerate() {
            if page.id() != first_id + index as u64 {
                return Err(Error::History("noncontiguous redo pages"));
            }
            let index = page.id() as usize - 1;
            if index == pages.len() {
                pages.push(page);
            } else if index + 1 == pages.len() {
                pages[index] = page;
            } else {
                return Err(Error::History("invalid redo target"));
            }
        }
    }
    if pages.is_empty() {
        return Err(Error::History("database creation was not committed"));
    }
    Ok(Snapshot::from_pages(pages)?)
}
