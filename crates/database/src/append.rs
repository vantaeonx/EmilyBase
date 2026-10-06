//! Canonical bounded append over an already validated immutable snapshot.
use super::Snapshot;
use crate::{Error, Event, MAX_APPEND_EVENTS, MAX_APPEND_PAGES, Result};
use emilybase_storage::{MAX_PAGES, Page};

impl Snapshot {
    /// Replay only new records while sharing prior immutable pages/row bodies.
    /// The input contains a changed last page and/or contiguous appended pages.
    /// All old slots and canonical event placement must match. No files are
    /// written; failure drops the private candidate and never mutates the base.
    pub fn replay_append_pages(&self, changed: &[Page]) -> Result<Self> {
        if changed.is_empty() {
            return Ok(self.clone());
        }
        if changed.len() > MAX_APPEND_PAGES {
            return Err(Error::Limit("history append pages"));
        }
        let old = self.pages.last().ok_or(Error::NotTableFile)?;
        let first = changed[0].id();
        let extending = first == old.id();
        if !extending && old.id().checked_add(1) != Some(first) {
            return Err(Error::Event("history append starts outside tail"));
        }
        let final_count = self
            .pages
            .len()
            .checked_add(changed.len() - usize::from(extending))
            .ok_or(Error::Limit("history append pages"))?;
        if final_count as u64 > MAX_PAGES {
            return Err(emilybase_storage::Error::PageLimit.into());
        }
        let mut events = 0usize;
        for (offset, page) in changed.iter().enumerate() {
            if first.checked_add(offset as u64) != Some(page.id())
                || page.record_count() == 0
                || page.record_count() != page.slot_count()
            {
                return Err(Error::Event("noncontiguous or deleted append slots"));
            }
            let start = if extending && offset == 0 {
                if page.slot_count() <= old.slot_count() {
                    return Err(Error::Event("history append does not extend tail"));
                }
                for slot in 0..old.slot_count() {
                    if old.get(slot as u16)? != page.get(slot as u16)? {
                        return Err(Error::Event("committed history rewrite"));
                    }
                }
                old.slot_count()
            } else {
                0
            };
            events = events
                .checked_add(page.slot_count() - start)
                .ok_or(Error::Limit("history append events"))?;
            if events > MAX_APPEND_EVENTS {
                return Err(Error::Limit("history append events"));
            }
        }
        // Normal event application detaches affected map structures once and
        // updates original derived index/location rules; unchanged bodies stay
        // shared. The base never exposes a mutable table/page to the candidate.
        let mut candidate = self.clone();
        for (offset, page) in changed.iter().enumerate() {
            let start = if extending && offset == 0 {
                old.slot_count()
            } else {
                0
            };
            for slot in start..page.slot_count() {
                candidate.apply(Event::decode(page.get(slot as u16)?)?)?;
            }
        }
        let start = usize::try_from(first - 1).map_err(|_| Error::Limit("history append pages"))?;
        if candidate.pages.len() != final_count
            || changed
                .iter()
                .zip(candidate.pages.iter().skip(start))
                .any(|(wanted, actual)| wanted.encode() != actual.encode())
        {
            return Err(Error::Event("noncanonical history append placement"));
        }
        Ok(candidate)
    }
}

#[cfg(test)]
#[path = "append_tests.rs"]
mod tests;
