use crate::page::Body;
use crate::{
    Error, IndexPage, Key, MAX_INDEX_ENTRIES, MAX_INDEX_PAGES, MAX_KEYS, MAX_TREE_HEIGHT, MIN_KEYS,
    PAGE_SIZE, RecordPointer, Result, validate_key,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Bounded original tree with linked leaves and shared immutable decoded pages.
/// Clones copy the bounded page map and retain page handles. Mutations stage a map
/// and replace changed pages; persistence and memory admission are external.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BPlusTree {
    pub(crate) pages: BTreeMap<u64, Arc<IndexPage>>,
    pub(crate) root: u64,
    pub(crate) len: usize,
    pub(crate) stable_ids: bool,
}

impl Default for BPlusTree {
    fn default() -> Self {
        Self::new()
    }
}
impl BPlusTree {
    pub fn new() -> Self {
        let root = IndexPage {
            id: 1,
            keys: Vec::new(),
            body: Body::Leaf {
                values: Vec::new(),
                next: None,
            },
        };
        Self {
            pages: BTreeMap::from([(1, Arc::new(root))]),
            root: 1,
            len: 0,
            stable_ids: false,
        }
    }
    /// Retain surviving arena IDs across deletion; freed IDs can be reused by later inserts.
    pub fn new_stable() -> Self {
        let mut tree = Self::new();
        tree.stable_ids = true;
        tree
    }
    pub fn has_stable_ids(&self) -> bool {
        self.stable_ids
    }
    /// Fully admit a stable-ID view without materializing all physical images.
    /// The private map is copied; immutable page bodies remain shared. Existing
    /// IDs/root/keys/pointers and wire bytes are preserved, including sparse IDs.
    /// Only the returned tree changes allocation/deletion policy.
    pub fn to_stable(&self) -> Result<Self> {
        if self.pages.is_empty() || self.pages.len() > MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        let mut admitted = self.clone();
        admitted.stable_ids = true;
        crate::IndexSnapshot::validate_tree(1, &admitted)?;
        Ok(admitted)
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn root_id(&self) -> u64 {
        self.root
    }
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub(crate) fn page(&self, id: u64) -> Result<&IndexPage> {
        self.pages
            .get(&id)
            .map(Arc::as_ref)
            .ok_or(Error::Layout("missing child"))
    }
    /// Retain an existing immutable page when a visited branch did not change.
    /// Equality includes keys, row pointers and every child/leaf link; an arena
    /// ID reused after reclamation must receive its new page contents.
    pub(crate) fn publish_page(&mut self, page: IndexPage) {
        if self.pages.get(&page.id).map(Arc::as_ref) != Some(&page) {
            self.pages.insert(page.id, Arc::new(page.compact_owned()));
        }
    }

    pub(crate) fn allocate(&mut self, keys: Vec<Key>, body: Body) -> Result<u64> {
        if self.pages.len() == MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        let id = if self.stable_ids {
            (1..=MAX_INDEX_PAGES as u64)
                .find(|id| !self.pages.contains_key(id))
                .ok_or(Error::Limit)?
        } else {
            self.pages.len() as u64 + 1
        };
        self.pages
            .insert(id, Arc::new(IndexPage { id, keys, body }.compact_owned()));
        Ok(id)
    }
    pub(crate) fn find_leaf(&self, key: Option<&Key>) -> Result<u64> {
        let mut id = self.root;
        for _ in 0..MAX_TREE_HEIGHT {
            let page = self.page(id)?;
            match &page.body {
                Body::Leaf { .. } => return Ok(id),
                Body::Branch { children } => {
                    let position = key.map_or(0, |key| {
                        page.keys.partition_point(|separator| separator <= key)
                    });
                    id = *children
                        .get(position)
                        .ok_or(Error::Layout("branch arity"))?;
                }
            }
        }
        Err(Error::Limit)
    }
    pub fn get(&self, key: &Key) -> Result<Option<RecordPointer>> {
        validate_key(key)?;
        let page = self.page(self.find_leaf(Some(key))?)?;
        match &page.body {
            Body::Leaf { values, .. } => Ok(page.keys.binary_search(key).ok().map(|i| values[i])),
            _ => Err(Error::Layout("expected leaf")),
        }
    }

    /// Unique insertion. Any error preserves the exact prior page images and root.
    pub fn insert(&mut self, key: Key, value: RecordPointer) -> Result<()> {
        validate_key(&key)?;
        if value.page_id == 0 {
            return Err(Error::PageId);
        }
        if self.get(&key)?.is_some() {
            return Err(Error::Duplicate);
        }
        if self.len == MAX_INDEX_ENTRIES {
            return Err(Error::Limit);
        }
        let mut staged = self.clone();
        if let Some((separator, right)) = staged.insert_at(staged.root, key, value, 0)? {
            staged.root = staged.allocate(
                vec![separator],
                Body::Branch {
                    children: vec![staged.root, right],
                },
            )?;
        }
        staged.len += 1;
        *self = staged;
        Ok(())
    }
    fn insert_at(
        &mut self,
        id: u64,
        key: Key,
        value: RecordPointer,
        depth: usize,
    ) -> Result<Option<(Key, u64)>> {
        if depth >= MAX_TREE_HEIGHT {
            return Err(Error::Limit);
        }
        let mut page = self.page(id)?.clone();
        match &mut page.body {
            Body::Leaf { values, .. } => {
                let position = page.keys.partition_point(|k| k < &key);
                page.keys.insert(position, key);
                values.insert(position, value);
            }
            Body::Branch { children } => {
                let position = page.keys.partition_point(|k| k <= &key);
                if let Some((separator, right)) =
                    self.insert_at(children[position], key, value, depth + 1)?
                {
                    page.keys.insert(position, separator);
                    children.insert(position + 1, right);
                }
            }
        }
        let split = if page.keys.len() > MAX_KEYS {
            let middle = page.keys.len() / 2;
            match &mut page.body {
                Body::Leaf { values, next } => {
                    let right_keys = page.keys.split_off(middle);
                    let separator = right_keys[0].clone();
                    let right = self.allocate(
                        right_keys,
                        Body::Leaf {
                            values: values.split_off(middle),
                            next: *next,
                        },
                    )?;
                    *next = Some(right);
                    Some((separator, right))
                }
                Body::Branch { children } => {
                    let right_keys = page.keys.split_off(middle + 1);
                    let separator = page.keys.pop().ok_or(Error::Layout("empty split"))?;
                    let right = self.allocate(
                        right_keys,
                        Body::Branch {
                            children: children.split_off(middle + 1),
                        },
                    )?;
                    Some((separator, right))
                }
            }
        } else {
            None
        };
        self.publish_page(page);
        Ok(split)
    }

    /// Ordered scan: inclusive start, exclusive end, bounded result allocation.
    pub fn range(
        &self,
        start: Option<&Key>,
        end: Option<&Key>,
        limit: usize,
    ) -> Result<Vec<(Key, RecordPointer)>> {
        if limit > MAX_INDEX_ENTRIES {
            return Err(Error::Limit);
        }
        for key in start.into_iter().chain(end) {
            validate_key(key)?;
        }
        if limit == 0 || matches!((start, end), (Some(a), Some(b)) if a >= b) {
            return Ok(Vec::new());
        }
        self.cursor(start, end)?
            .take(limit)
            .map(|entry| entry.map(|(key, pointer)| (key.clone(), pointer)))
            .collect()
    }
    pub fn page_images(&self) -> Result<Vec<[u8; PAGE_SIZE]>> {
        self.pages.values().map(|page| page.encode()).collect()
    }

    /// Import dense page IDs with complete topology, separator and leaf-chain validation.
    pub fn from_pages(root: u64, images: &[[u8; PAGE_SIZE]]) -> Result<Self> {
        if images.is_empty() || images.len() > MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        let pages = images
            .iter()
            .enumerate()
            .map(|(i, image)| {
                let id = i as u64 + 1;
                Ok((id, Arc::new(IndexPage::decode(image, id)?)))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut tree = Self {
            pages,
            root,
            len: 0,
            stable_ids: false,
        };
        tree.len = tree.validate()?;
        Ok(tree)
    }
    /// Import canonical sparse images whose embedded IDs are bounded and strictly increasing.
    pub fn from_stable_pages(root: u64, images: &[[u8; PAGE_SIZE]]) -> Result<Self> {
        if images.is_empty() || images.len() > MAX_INDEX_PAGES {
            return Err(Error::Limit);
        }
        let mut pages = BTreeMap::new();
        let mut previous = 0;
        for image in images {
            let id = u64::from_le_bytes(image[8..16].try_into().map_err(|_| Error::PageId)?);
            if id <= previous || id > MAX_INDEX_PAGES as u64 {
                return Err(Error::PageId);
            }
            pages.insert(id, Arc::new(IndexPage::decode(image, id)?));
            previous = id;
        }
        let mut tree = Self {
            pages,
            root,
            len: 0,
            stable_ids: true,
        };
        tree.len = tree.validate()?;
        Ok(tree)
    }
    pub fn validate(&self) -> Result<usize> {
        let mut visited = BTreeSet::new();
        let mut leaves = Vec::new();
        let mut leaf_depth = None;
        let summary = self.walk(self.root, 0, &mut visited, &mut leaves, &mut leaf_depth)?;
        if visited.len() != self.pages.len() {
            return Err(Error::Layout("unreachable pages"));
        }
        for (i, id) in leaves.iter().enumerate() {
            if self.page(*id)?.next_leaf() != leaves.get(i + 1).copied() {
                return Err(Error::Layout("leaf chain disagrees with tree"));
            }
        }
        Ok(summary.count)
    }
    fn walk(
        &self,
        id: u64,
        depth: usize,
        visited: &mut BTreeSet<u64>,
        leaves: &mut Vec<u64>,
        leaf_depth: &mut Option<usize>,
    ) -> Result<Summary> {
        if depth >= MAX_TREE_HEIGHT {
            return Err(Error::Limit);
        }
        if !visited.insert(id) {
            return Err(Error::Layout("cycle or shared child"));
        }
        let page = self.page(id)?;
        page.validate()?;
        if id != self.root && page.keys.len() < MIN_KEYS {
            return Err(Error::Layout("underfull child"));
        }
        match &page.body {
            Body::Leaf { .. } => {
                if leaf_depth.is_some_and(|expected| expected != depth) {
                    return Err(Error::Layout("unbalanced leaves"));
                }
                *leaf_depth = Some(depth);
                leaves.push(id);
                Ok(Summary {
                    min: page.keys.first().cloned(),
                    max: page.keys.last().cloned(),
                    count: page.keys.len(),
                })
            }
            Body::Branch { children } => {
                if page.keys.is_empty() {
                    return Err(Error::Layout("empty branch"));
                }
                let mut combined =
                    self.walk(children[0], depth + 1, visited, leaves, leaf_depth)?;
                for (separator, child) in page.keys.iter().zip(&children[1..]) {
                    let right = self.walk(*child, depth + 1, visited, leaves, leaf_depth)?;
                    if right.min.as_ref() != Some(separator)
                        || combined.max.as_ref().is_none_or(|max| max >= separator)
                    {
                        return Err(Error::Layout("separator or child key range"));
                    }
                    combined.max = right.max;
                    combined.count += right.count;
                    if combined.count > MAX_INDEX_ENTRIES {
                        return Err(Error::Limit);
                    }
                }
                Ok(combined)
            }
        }
    }
}
struct Summary {
    min: Option<Key>,
    max: Option<Key>,
    count: usize,
}
