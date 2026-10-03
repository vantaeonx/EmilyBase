use std::collections::BTreeMap;

use crate::page::Body;
use crate::{
    BPlusTree, Error, IndexPage, Key, MAX_TREE_HEIGHT, MIN_KEYS, RecordPointer, Result,
    validate_key,
};

impl BPlusTree {
    /// Replace a row pointer without changing keys, routing, root or arena IDs.
    pub fn replace(&mut self, key: &Key, value: RecordPointer) -> Result<RecordPointer> {
        validate_key(key)?;
        if value.page_id == 0 {
            return Err(Error::PageId);
        }
        let id = self.find_leaf(Some(key))?;
        let mut page = self.page(id)?.clone();
        let position = page.keys.binary_search(key).map_err(|_| Error::NoKey)?;
        let Body::Leaf { values, .. } = &mut page.body else {
            return Err(Error::Layout("expected leaf"));
        };
        let old = std::mem::replace(&mut values[position], value);
        page.validate()?;
        self.pages.insert(id, page);
        Ok(old)
    }

    /// Delete a key with sibling rotations/merges. Any failure preserves all old images.
    /// Dense mode renumbers arena IDs; stable mode preserves every surviving page ID.
    pub fn remove(&mut self, key: &Key) -> Result<RecordPointer> {
        validate_key(key)?;
        let old = self.get(key)?.ok_or(Error::NoKey)?;
        let mut staged = self.clone();
        staged.remove_at(staged.root, key, 0)?;
        let root = staged.page(staged.root)?.clone();
        if let Body::Branch { children } = root.body
            && children.len() == 1
        {
            staged.pages.remove(&staged.root);
            staged.root = children[0];
        }
        staged.len -= 1;
        if !staged.stable_ids {
            staged.densify()?;
        }
        if staged.validate()? != staged.len {
            return Err(Error::Layout("delete entry count"));
        }
        *self = staged;
        Ok(old)
    }

    fn remove_at(&mut self, id: u64, key: &Key, depth: usize) -> Result<()> {
        if depth >= MAX_TREE_HEIGHT {
            return Err(Error::Limit);
        }
        let mut page = self.page(id)?.clone();
        match &mut page.body {
            Body::Leaf { values, .. } => {
                let position = page.keys.binary_search(key).map_err(|_| Error::NoKey)?;
                page.keys.remove(position);
                values.remove(position);
            }
            Body::Branch { children } => {
                let position = page.keys.partition_point(|separator| separator <= key);
                self.remove_at(children[position], key, depth + 1)?;
                if children.len() > 1 && self.page(children[position])?.keys.len() < MIN_KEYS {
                    self.rebalance(children, position)?;
                }
            }
        }
        self.refresh(&mut page)?;
        self.pages.insert(id, page);
        Ok(())
    }

    fn minimum(&self, mut id: u64) -> Result<Key> {
        for _ in 0..MAX_TREE_HEIGHT {
            let page = self.page(id)?;
            match &page.body {
                Body::Leaf { .. } => {
                    return page
                        .keys
                        .first()
                        .cloned()
                        .ok_or(Error::Layout("empty child"));
                }
                Body::Branch { children } => {
                    id = *children.first().ok_or(Error::Layout("empty branch"))?
                }
            }
        }
        Err(Error::Limit)
    }

    fn refresh(&self, page: &mut IndexPage) -> Result<()> {
        if let Body::Branch { children } = &page.body {
            page.keys = children
                .get(1..)
                .ok_or(Error::Layout("empty branch"))?
                .iter()
                .map(|id| self.minimum(*id))
                .collect::<Result<_>>()?;
        }
        Ok(())
    }

    fn rebalance(&mut self, children: &mut Vec<u64>, position: usize) -> Result<()> {
        if position > 0 && self.page(children[position - 1])?.keys.len() > MIN_KEYS {
            return self.rotate(children[position - 1], children[position], true);
        }
        if position + 1 < children.len() && self.page(children[position + 1])?.keys.len() > MIN_KEYS
        {
            return self.rotate(children[position], children[position + 1], false);
        }
        let left = position.saturating_sub(1);
        self.merge(children[left], children[left + 1])?;
        children.remove(left + 1);
        Ok(())
    }

    /// Rotate one key/pointer or child link; separators are recomputed from subtree minima.
    fn rotate(&mut self, left_id: u64, right_id: u64, from_left: bool) -> Result<()> {
        let mut left = self.page(left_id)?.clone();
        let mut right = self.page(right_id)?.clone();
        match (&mut left.body, &mut right.body) {
            (Body::Leaf { values: lv, .. }, Body::Leaf { values: rv, .. }) => {
                if from_left {
                    right
                        .keys
                        .insert(0, left.keys.pop().ok_or(Error::Layout("empty donor"))?);
                    rv.insert(0, lv.pop().ok_or(Error::Layout("empty donor"))?);
                } else {
                    left.keys.push(right.keys.remove(0));
                    lv.push(rv.remove(0));
                }
            }
            (Body::Branch { children: lc }, Body::Branch { children: rc }) => {
                if from_left {
                    rc.insert(0, lc.pop().ok_or(Error::Layout("empty donor"))?);
                } else {
                    lc.push(rc.remove(0));
                }
            }
            _ => return Err(Error::Layout("sibling kinds")),
        }
        self.refresh(&mut left)?;
        self.refresh(&mut right)?;
        self.pages.insert(left_id, left);
        self.pages.insert(right_id, right);
        Ok(())
    }

    fn merge(&mut self, left_id: u64, right_id: u64) -> Result<()> {
        let mut left = self.page(left_id)?.clone();
        let right = self.page(right_id)?.clone();
        match (&mut left.body, right.body) {
            (
                Body::Leaf { values, next },
                Body::Leaf {
                    values: rv,
                    next: rn,
                },
            ) => {
                left.keys.extend(right.keys);
                values.extend(rv);
                *next = rn;
            }
            (Body::Branch { children }, Body::Branch { children: rc }) => children.extend(rc),
            _ => return Err(Error::Layout("sibling kinds")),
        }
        self.refresh(&mut left)?;
        self.pages.insert(left_id, left);
        self.pages.remove(&right_id);
        Ok(())
    }

    /// Keep the existing version-1 dense-image contract after reclaiming merged nodes.
    fn densify(&mut self) -> Result<()> {
        let ids: BTreeMap<_, _> = self
            .pages
            .keys()
            .enumerate()
            .map(|(i, old)| (*old, i as u64 + 1))
            .collect();
        let map_id = |old: u64| {
            ids.get(&old)
                .copied()
                .ok_or(Error::Layout("reclaimed page still referenced"))
        };
        let mut pages = BTreeMap::new();
        for source in self.pages.values() {
            let mut page = source.clone();
            page.id = map_id(page.id)?;
            match &mut page.body {
                Body::Leaf { next, .. } => *next = next.map(map_id).transpose()?,
                Body::Branch { children } => {
                    for id in children {
                        *id = map_id(*id)?;
                    }
                }
            }
            pages.insert(page.id, page);
        }
        self.root = map_id(self.root)?;
        self.pages = pages;
        Ok(())
    }
}
