use emilybase_commit_format::{IndexKeyType, PageAddress, Predecessor, RootBinding};
use proptest::prelude::*;
use std::collections::BTreeSet;

proptest! {
    #[test]
    fn independent_namespace_tuples_remain_distinct(
        number in any::<u64>(), table in 1u64..=u64::MAX, page in 1u64..=1024,
    ) {
        let mut database = [1; 16];
        database[8..].copy_from_slice(&number.to_le_bytes());
        let mut other = database;
        other[0] = 2;
        let next_table = if table == u64::MAX { 1 } else { table + 1 };
        let addresses = [
            PageAddress::history(database, page).unwrap(),
            PageAddress::primary(database, table, page).unwrap(),
            PageAddress::primary(database, next_table, page).unwrap(),
            PageAddress::primary(other, table, page).unwrap(),
        ];
        let tuples = [(database, 1u8, 0, page), (database, 2, table, page),
            (database, 2, next_table, page), (other, 2, table, page)];
        let decoded = addresses.into_iter().map(|value| PageAddress::decode(&value.encode().unwrap()).unwrap())
            .collect::<BTreeSet<_>>();
        prop_assert_eq!(decoded.len(), tuples.into_iter().collect::<BTreeSet<_>>().len());
        for (address, (database, domain, table, page)) in addresses.into_iter().zip(tuples) {
            prop_assert_eq!((address.database(), address.domain() as u8, address.table(), address.page()),
                (database, domain, table, page));
        }
    }

    #[test]
    fn root_revisions_bind_exact_prior_transactions_and_fingerprints(
        table in 1u64..=u64::MAX, page in 1u64..=1024,
        base_revision in 1u64..u64::MAX, base_transaction in 1u64..(u64::MAX-2),
        gap in 1u64..10000, covered in 0u64..=10000, excluded in 0u64..=10000,
        text in any::<bool>(), fingerprint in prop::array::uniform32(any::<u8>()),
    ) {
        let revision = base_revision + 1;
        let transaction = base_transaction + gap.min(u64::MAX - 1 - base_transaction);
        let key_type = if text { IndexKeyType::Text } else { IndexKeyType::Integer };
        let excluded = if text { excluded % (10001-covered) } else { 0 };
        let previous_base = if base_revision == 1 { None } else {
            Some(Predecessor::new(base_revision-1, base_transaction-1, [3;32]))
        };
        // An older revision needs an older owning transaction. Refuse impossible pairs.
        if base_revision > 1 && base_transaction == 1 {
            return Ok(());
        }
        let previous_base = previous_base.transpose().unwrap();
        let previous = RootBinding::new(PageAddress::primary([1;16],table,1).unwrap(),
            key_type,base_revision,base_transaction,0,0,1,previous_base).unwrap();
        let root = RootBinding::new(PageAddress::primary([1;16],table,page).unwrap(),
            key_type,revision,transaction,covered,excluded,1,
            Some(Predecessor::new(base_revision,base_transaction,fingerprint).unwrap())).unwrap();
        let decoded = RootBinding::decode(&root.encode().unwrap()).unwrap();
        prop_assert_eq!(decoded,root);
        prop_assert!(decoded.verify_predecessor(previous,fingerprint).is_ok());
        let mut other = fingerprint;
        other[0] ^= 1;
        prop_assert!(decoded.verify_predecessor(previous,other).is_err());
        prop_assert!(decoded.verify_owner([1;16],table,transaction).is_ok());
        prop_assert!(decoded.verify_owner([1;16],table,base_transaction).is_err());
        prop_assert_eq!((decoded.covered(),decoded.excluded()),(covered,excluded));
    }

    #[test]
    fn arbitrary_external_bytes_never_panic_or_decode_noncanonical_metadata(
        bytes in prop::collection::vec(any::<u8>(),0..384),
    ) {
        if let Ok(address) = PageAddress::decode(&bytes) {
            let encoded = address.encode().unwrap();
            prop_assert_eq!(encoded.as_slice(),bytes.as_slice());
        }
        if let Ok(root) = RootBinding::decode(&bytes) {
            let encoded = root.encode().unwrap();
            prop_assert_eq!(encoded.as_slice(),bytes.as_slice());
        }
    }
}
