use super::*;

#[test]
fn aggregate_charges_accept_exact_limits_and_refuse_overflow() {
    for limit in [MAX_COMPONENT_OBJECTS, MAX_COMPONENT_IMAGE_BYTES] {
        assert_eq!(charge(0, limit, limit).unwrap(), limit);
        assert_eq!(charge(limit - 1, 1, limit).unwrap(), limit);
        assert!(matches!(charge(limit, 1, limit), Err(Error::Limit)));
        assert!(matches!(charge(usize::MAX, 1, limit), Err(Error::Limit)));
        assert!(matches!(charge(0, limit + 1, limit), Err(Error::Limit)));
    }
}

#[test]
fn component_database_identity_domains_are_globally_disjoint() {
    let ids = [[1; 16], [2; 16], [3; 16], [4; 16], [5; 16], [6; 16]];
    unique_ids(ids.into_iter()).unwrap();
    for left in 0..ids.len() {
        for right in left + 1..ids.len() {
            let mut duplicate = ids;
            duplicate[right] = duplicate[left];
            assert!(matches!(
                unique_ids(duplicate.into_iter()),
                Err(Error::ComponentSnapshot(_))
            ));
        }
    }
}

#[test]
fn canonical_binding_identity_matches_native_lowercase_hex() {
    for id in [[0; 16], [255; 16], std::array::from_fn(|index| index as u8)] {
        let binding = FileRootBinding::new(&hex(&id), &hex(&id)).unwrap();
        assert_eq!(binding.database_id().len(), 32);
        let native: emilybase_object_storage::ProjectId = binding.project().parse().unwrap();
        assert_eq!(native.as_bytes(), &id);
    }
}
