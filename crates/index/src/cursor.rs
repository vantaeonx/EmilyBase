use crate::page::Body;
use crate::{
    BPlusTree, Error, Key, MAX_INDEX_ENTRIES, MAX_INDEX_PAGES, MAX_TREE_HEIGHT, RecordPointer,
    Result, validate_key,
};

#[derive(Clone, Copy)]
struct Frame {
    branch: u64,
    child: usize,
}
struct Position {
    leaf: u64,
    slot: usize,
    path: Vec<Frame>,
}

/// Borrowed, fused, double-ended interval in key order. Bounds are owned and small.
/// No rows, page images or result key vectors are copied. Errors retire both ends.
pub struct RangeCursor<'a> {
    tree: &'a BPlusTree,
    lower: Option<Key>,
    upper: Option<Key>,
    front: Option<Position>,
    back: Option<Position>,
    last_front: Option<&'a Key>,
    last_back: Option<&'a Key>,
    emitted: usize,
    front_leaves: usize,
    back_leaves: usize,
    done: bool,
}

impl BPlusTree {
    /// Inclusive lower, exclusive upper. Validate bounds even for an empty interval.
    /// Descending iteration follows bounded ancestor paths; EBIX-1 needs no prev link.
    pub fn cursor(&self, lower: Option<&Key>, upper: Option<&Key>) -> Result<RangeCursor<'_>> {
        for key in lower.into_iter().chain(upper) {
            validate_key(key)?;
        }
        if self.len > MAX_INDEX_ENTRIES || self.pages.len() > MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        let done = self.is_empty() || matches!((lower, upper), (Some(a), Some(b)) if a >= b);
        let (front, back) = if done {
            (None, None)
        } else {
            (
                Some(seek(self, self.root, Vec::new(), lower, false)?),
                Some(seek(self, self.root, Vec::new(), upper, true)?),
            )
        };
        Ok(RangeCursor {
            tree: self,
            lower: lower.cloned(),
            upper: upper.cloned(),
            front,
            back,
            last_front: None,
            last_back: None,
            emitted: 0,
            front_leaves: 0,
            back_leaves: 0,
            done,
        })
    }
}

fn seek(
    tree: &BPlusTree,
    mut id: u64,
    mut path: Vec<Frame>,
    bound: Option<&Key>,
    backwards: bool,
) -> Result<Position> {
    loop {
        if path.len() >= MAX_TREE_HEIGHT {
            return Err(Error::Limit);
        }
        if path.iter().any(|frame| frame.branch == id) {
            return Err(Error::Layout("cursor ancestor cycle"));
        }
        let page = tree.page(id)?;
        page.validate()?;
        match &page.body {
            Body::Leaf { .. } => {
                let slot = bound.map_or(if backwards { page.keys.len() } else { 0 }, |key| {
                    page.keys.partition_point(|candidate| candidate < key)
                });
                return Ok(Position {
                    leaf: id,
                    slot,
                    path,
                });
            }
            Body::Branch { children } => {
                let child = bound.map_or(if backwards { children.len() - 1 } else { 0 }, |key| {
                    page.keys.partition_point(|separator| separator <= key)
                });
                path.push(Frame { branch: id, child });
                id = *children
                    .get(child)
                    .ok_or(Error::Layout("cursor branch arity"))?;
            }
        }
    }
}

fn adjacent(tree: &BPlusTree, old: Position, backwards: bool) -> Result<Option<Position>> {
    let old_leaf = old.leaf;
    let mut path = old.path;
    while let Some(mut frame) = path.pop() {
        let branch = tree.page(frame.branch)?;
        branch.validate()?;
        let Body::Branch { children } = &branch.body else {
            return Err(Error::Layout("cursor ancestor is not a branch"));
        };
        let child = if backwards {
            frame.child.checked_sub(1)
        } else {
            frame
                .child
                .checked_add(1)
                .filter(|next| *next < children.len())
        };
        if let Some(child) = child {
            let id = *children
                .get(child)
                .ok_or(Error::Layout("cursor child missing"))?;
            frame.child = child;
            path.push(frame);
            let next = seek(tree, id, path, None, backwards)?;
            let (left, right) = if backwards {
                (next.leaf, old_leaf)
            } else {
                (old_leaf, next.leaf)
            };
            if tree.page(left)?.next_leaf() != Some(right) {
                return Err(Error::Layout("cursor leaf link mismatch"));
            }
            return Ok(Some(next));
        }
    }
    if !backwards && tree.page(old_leaf)?.next_leaf().is_some() {
        return Err(Error::Layout("cursor terminal leaf link"));
    }
    Ok(None)
}

impl<'a> RangeCursor<'a> {
    fn advance(&mut self, backwards: bool) -> Result<Option<(&'a Key, RecordPointer)>> {
        let position = if backwards {
            &mut self.back
        } else {
            &mut self.front
        };
        loop {
            let Some(current) = position.as_mut() else {
                return Ok(None);
            };
            let page = self.tree.page(current.leaf)?;
            page.validate()?;
            let Body::Leaf { values, .. } = &page.body else {
                return Err(Error::Layout("cursor position is not a leaf"));
            };
            let index = if backwards {
                current.slot.checked_sub(1)
            } else if current.slot < page.keys.len() {
                Some(current.slot)
            } else {
                None
            };
            let Some(index) = index else {
                let leaves = if backwards {
                    &mut self.back_leaves
                } else {
                    &mut self.front_leaves
                };
                *leaves += 1;
                if *leaves > MAX_INDEX_PAGES {
                    return Err(Error::Limit);
                }
                let old = position
                    .take()
                    .ok_or(Error::Layout("cursor position missing"))?;
                *position = adjacent(self.tree, old, backwards)?;
                continue;
            };
            let key = page.keys.get(index).ok_or(Error::Layout("cursor slot"))?;
            let value = *values.get(index).ok_or(Error::Layout("cursor pointer"))?;
            if backwards {
                current.slot = index;
            } else {
                current.slot += 1;
            }
            if self.lower.as_ref().is_some_and(|bound| key < bound)
                || self.upper.as_ref().is_some_and(|bound| key >= bound)
                || self.last_front.is_some_and(|front| key <= front) && backwards
                || self.last_back.is_some_and(|back| key >= back) && !backwards
            {
                return Ok(None);
            }
            let last = if backwards {
                &mut self.last_back
            } else {
                &mut self.last_front
            };
            if last.is_some_and(|last| if backwards { key >= last } else { key <= last }) {
                return Err(Error::Layout("cursor key order"));
            }
            if self.emitted >= self.tree.len {
                return Err(Error::Layout("cursor entry count"));
            }
            *last = Some(key);
            self.emitted += 1;
            return Ok(Some((key, value)));
        }
    }

    fn item(&mut self, backwards: bool) -> Option<Result<(&'a Key, RecordPointer)>> {
        if self.done {
            return None;
        }
        match self.advance(backwards) {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}

impl<'a> Iterator for RangeCursor<'a> {
    type Item = Result<(&'a Key, RecordPointer)>;
    fn next(&mut self) -> Option<Self::Item> {
        self.item(false)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            0,
            Some(if self.done {
                0
            } else {
                self.tree.len.saturating_add(1).saturating_sub(self.emitted)
            }),
        )
    }
}
impl DoubleEndedIterator for RangeCursor<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.item(true)
    }
}
impl std::iter::FusedIterator for RangeCursor<'_> {}
