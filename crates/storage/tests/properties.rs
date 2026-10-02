use emilybase_storage::{Error, PAGE_SIZE, Page, header};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn operations_match_a_record_model(
        operations in prop::collection::vec(
            (0u8..3, 0u16..40, prop::collection::vec(any::<u8>(), 0..160)), 0..80
        )
    ) {
        let mut page = Page::new(1).unwrap();
        let mut model: Vec<Option<Vec<u8>>> = Vec::new();
        for (operation, slot, payload) in operations {
            let before = page.encode();
            match operation {
                0 => match page.insert(&payload) {
                    Ok(actual) => {
                        let expected = model.iter().position(Option::is_none).unwrap_or(model.len());
                        prop_assert_eq!(usize::from(actual), expected);
                        if expected == model.len() {
                            model.push(Some(payload));
                        } else {
                            model[expected] = Some(payload);
                        }
                    }
                    Err(error) => {
                        prop_assert!(matches!(error, Error::PageFull));
                        prop_assert_eq!(page.encode(), before);
                    }
                },
                1 => {
                    let existing = model.get(usize::from(slot)).and_then(Option::as_ref);
                    let used: usize = model.iter().flatten().map(Vec::len).sum();
                    let available = PAGE_SIZE - 32 - 6 * model.len() - used;
                    let should_succeed = existing.is_some_and(|old| payload.len() <= available + old.len());
                    prop_assert_eq!(page.update(slot, &payload).is_ok(), should_succeed);
                    if should_succeed {
                        model[usize::from(slot)] = Some(payload);
                    } else {
                        prop_assert_eq!(page.encode(), before);
                    }
                },
                _ => {
                    let exists = model.get(usize::from(slot)).is_some_and(Option::is_some);
                    prop_assert_eq!(page.delete(slot).is_ok(), exists);
                    if exists { model[usize::from(slot)] = None; }
                }
            }
            page = Page::decode(&page.encode(), 1).unwrap();
            prop_assert_eq!(page.record_count(), model.iter().flatten().count());
            prop_assert_eq!(page.slot_count(), model.len());
            for (slot, expected) in model.iter().enumerate() {
                prop_assert_eq!(page.get(slot as u16).ok(), expected.as_deref());
            }
        }
    }

    #[test]
    fn arbitrary_input_does_not_panic(bytes in prop::collection::vec(any::<u8>(), 0..5000)) {
        let _ = Page::decode(&bytes, 1);
        let _ = header::decode(&bytes);
    }

    #[test]
    fn mutations_with_valid_checksum_are_structurally_checked(
        offset in 0usize..PAGE_SIZE,
        value in any::<u8>(),
    ) {
        let mut page = Page::new(1).unwrap();
        page.insert(b"first").unwrap();
        page.insert(b"").unwrap();
        page.insert(b"second").unwrap();
        let mut bytes = page.encode();
        bytes[offset] = value;
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&bytes[..28]);
        hasher.update(&bytes[32..]);
        bytes[28..32].copy_from_slice(&hasher.finalize().to_le_bytes());
        if let Ok(decoded) = Page::decode(&bytes, 1) {
            prop_assert_eq!(Page::decode(&decoded.encode(), 1).unwrap(), decoded);
        }
    }
}
