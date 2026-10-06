use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use crate::page::Body;
use crate::{
    BPlusTree, Error, IndexPage, Key, MAX_INDEX_ENTRIES, MAX_KEYS, RecordPointer, Result,
    validate_key,
};

impl BPlusTree {
    /// Build balanced leaves/branches bottom-up from strictly sorted unique entries.
    /// The borrowed source is never changed, and all limits are checked before building.
    pub fn from_sorted(entries: &[(Key, RecordPointer)]) -> Result<Self> {
        if entries.len() > MAX_INDEX_ENTRIES {
            return Err(Error::Limit);
        }
        for (key, pointer) in entries {
            validate_key(key)?;
            if pointer.page_id == 0 {
                return Err(Error::PageId);
            }
        }
        for pair in entries.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(Error::Duplicate);
            }
            if pair[0].0 > pair[1].0 {
                return Err(Error::Layout("bulk input order"));
            }
        }
        if entries.is_empty() {
            return Ok(Self::new());
        }
        let groups = balanced_groups(entries.len(), MAX_KEYS);
        let mut tree = Self {
            pages: BTreeMap::new(),
            root: 1,
            len: entries.len(),
            stable_ids: false,
        };
        let mut level = Vec::with_capacity(groups.len());
        for (position, range) in groups.iter().enumerate() {
            let id = position as u64 + 1;
            let next = (position + 1 < groups.len()).then_some(id + 1);
            let minimum = entries[range.start].0.clone();
            let page = IndexPage::leaf(id, entries[range.clone()].to_vec(), next)?;
            tree.pages.insert(id, Arc::new(page));
            level.push((id, minimum));
        }
        while level.len() > 1 {
            let mut parents = Vec::new();
            for range in balanced_groups(level.len(), MAX_KEYS + 1) {
                let children = &level[range];
                let minimum = children[0].1.clone();
                let keys = children[1..].iter().map(|child| child.1.clone()).collect();
                let id = tree.allocate(
                    keys,
                    Body::Branch {
                        children: children.iter().map(|child| child.0).collect(),
                    },
                )?;
                parents.push((id, minimum));
            }
            level = parents;
        }
        tree.root = level[0].0;
        if tree.validate()? != entries.len() {
            return Err(Error::Layout("bulk entry count"));
        }
        Ok(tree)
    }
    /// Balanced bulk build with stable IDs for later mutation/snapshot publication.
    pub fn from_sorted_stable(entries: &[(Key, RecordPointer)]) -> Result<Self> {
        let mut tree = Self::from_sorted(entries)?;
        tree.stable_ids = true;
        Ok(tree)
    }
}

/// Spread remainder over all nodes: a nearly empty final leaf/branch is never emitted.
fn balanced_groups(total: usize, capacity: usize) -> Vec<Range<usize>> {
    let groups = total.div_ceil(capacity);
    let base = total / groups;
    let remainder = total % groups;
    let mut start = 0;
    (0..groups)
        .map(|i| {
            let end = start + base + usize::from(i < remainder);
            let range = start..end;
            start = end;
            range
        })
        .collect()
}
