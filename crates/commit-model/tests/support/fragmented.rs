use emilybase_index::{BPlusTree, IndexPage, Key, RecordPointer};

// Independent minimum-leaf-occupancy fixture. It does not call the bulk loader.
pub fn tree(entries: &[(Key, RecordPointer)], minimum_branches: bool) -> BPlusTree {
    if entries.is_empty() {
        return BPlusTree::new_stable();
    }
    let groups = (entries.len() / 7).max(1);
    let narrow = entries.len() / groups;
    let wide = entries.len() % groups;
    let mut images = Vec::new();
    let mut level = Vec::new();
    let mut offset = 0;
    for group in 0..groups {
        let count = narrow + usize::from(group < wide);
        let id = group as u64 + 1;
        let children = &entries[offset..offset + count];
        let next = (group + 1 < groups).then_some(id + 1);
        images.push(
            IndexPage::leaf(id, children.to_vec(), next)
                .unwrap()
                .encode()
                .unwrap(),
        );
        level.push((children[0].0.clone(), id));
        offset += count;
    }
    assert_eq!(offset, entries.len());
    let mut next_id = groups as u64 + 1;
    while level.len() > 1 {
        let groups = if minimum_branches {
            (level.len() / 8).max(1)
        } else {
            level.len().div_ceil(15)
        };
        let narrow = level.len() / groups;
        let wide = level.len() % groups;
        let mut next = Vec::new();
        let mut offset = 0;
        for group in 0..groups {
            let count = narrow + usize::from(group < wide);
            let children = &level[offset..offset + count];
            images.push(
                IndexPage::branch(
                    next_id,
                    children[1..]
                        .iter()
                        .map(|(minimum, _)| minimum.clone())
                        .collect(),
                    children.iter().map(|(_, id)| *id).collect(),
                )
                .unwrap()
                .encode()
                .unwrap(),
            );
            next.push((children[0].0.clone(), next_id));
            next_id += 1;
            offset += count;
        }
        assert_eq!(offset, level.len());
        level = next;
    }
    BPlusTree::from_stable_pages(level[0].1, &images).unwrap()
}
