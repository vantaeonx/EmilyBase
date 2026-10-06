use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_commit_format::{PageAddress, ROOT_BYTES};
use emilybase_commit_model::{
    Error, IMAGE_PLAN_HEADER_BYTES, IMAGE_PLAN_MAX_BYTES, IMAGE_PLAN_VERSION, ImagePlan, Model,
    PlanCounts,
};
use emilybase_database::{Event, EventKind};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
mod support;
use support::{model, schema};

fn repair(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let digest = Sha256::digest(&bytes[..end]);
    bytes[end..].copy_from_slice(&digest);
}

fn plan(base: &Model, commands: &[(i64, &str)]) -> ImagePlan {
    let mut staged = base.begin().unwrap();
    for (key, value) in commands {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(*key), Value::Text((*value).into())]),
            })
            .unwrap();
    }
    staged.rebuild_index("items").unwrap();
    staged.prepare().unwrap().image_plan().unwrap()
}

fn root_offset(bytes: &[u8]) -> usize {
    let pages = u32::from_le_bytes(bytes[112..116].try_into().unwrap()) as usize;
    IMAGE_PLAN_HEADER_BYTES + pages * (64 + 4096)
}

#[test]
fn envelope_has_exact_combined_size_and_keeps_original_component_bytes() {
    let base = model(&[("items", DataType::Integer)]);
    let original = plan(&base, &[(i64::MIN, "minimum"), (i64::MAX, "maximum")]);
    let bytes = original.encode().unwrap();
    assert_eq!(
        original.counts().unwrap().envelope_bytes().unwrap(),
        bytes.len() as u64
    );
    assert_eq!(&bytes[..8], b"EBIP\0\0\0\0");
    assert_eq!(
        u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
        IMAGE_PLAN_VERSION
    );
    assert_eq!(
        &bytes[192..256],
        &original.history()[0].address().encode().unwrap()
    );
    assert_eq!(&bytes[256..4352], original.history()[0].image());
    let decoded = ImagePlan::decode(&bytes).unwrap();
    assert_eq!(decoded.encode().unwrap(), bytes);
    assert_eq!(decoded.counts().unwrap(), original.counts().unwrap());
    assert_eq!(
        decoded.replay(&base).unwrap().fingerprint(),
        original.next_fingerprint()
    );
    for key in [i64::MIN, i64::MAX] {
        assert!(
            decoded
                .replay(&base)
                .unwrap()
                .view()
                .get("items", &Key::Integer(key))
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(base.view().row_count(), 0);
}

#[test]
fn every_byte_cut_damage_and_extra_suffix_is_refused() {
    let base = model(&[("items", DataType::Integer)]);
    let bytes = plan(&base, &[(1, "synthetic")]).encode().unwrap();
    for end in 0..bytes.len() {
        assert!(ImagePlan::decode(&bytes[..end]).is_err(), "cut {end}");
    }
    for offset in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(ImagePlan::decode(&changed).is_err(), "damage {offset}");
    }
    let mut extra = bytes;
    extra.push(0);
    repair(&mut extra);
    assert!(matches!(ImagePlan::decode(&extra), Err(Error::PlanLength)));
    assert_eq!(base.view().row_count(), 0);
}

#[test]
fn repaired_header_still_refuses_versions_reserved_counts_lengths_and_transactions() {
    let base = model(&[("items", DataType::Integer)]);
    let original = plan(&base, &[(1, "value")]).encode().unwrap();
    for offset in [12, 13, 15, 140, 191] {
        let mut bytes = original.clone();
        bytes[offset] = 1;
        repair(&mut bytes);
        assert!(matches!(
            ImagePlan::decode(&bytes),
            Err(Error::Plan("envelope reserved fields"))
        ));
    }
    for version in [0u16, 2, u16::MAX] {
        let mut bytes = original.clone();
        bytes[8..10].copy_from_slice(&version.to_le_bytes());
        repair(&mut bytes);
        assert!(
            matches!(ImagePlan::decode(&bytes), Err(Error::PlanVersion(value)) if value == version)
        );
    }
    for offset in [112, 116, 120, 124, 128] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        repair(&mut bytes);
        assert!(matches!(ImagePlan::decode(&bytes), Err(Error::Limit)));
    }
    for offset in [32, 40, 132] {
        for number in [0u64, u64::MAX] {
            let mut bytes = original.clone();
            bytes[offset..offset + 8].copy_from_slice(&number.to_le_bytes());
            repair(&mut bytes);
            assert!(ImagePlan::decode(&bytes).is_err());
        }
    }
    let mut zero_database = original.clone();
    zero_database[16..32].fill(0);
    repair(&mut zero_database);
    assert!(ImagePlan::decode(&zero_database).is_err());
    let mut bad_width = original.clone();
    bad_width[10..12].copy_from_slice(&193u16.to_le_bytes());
    repair(&mut bad_width);
    assert!(matches!(
        ImagePlan::decode(&bad_width),
        Err(Error::PlanLength)
    ));
    assert!(matches!(
        ImagePlan::decode(&vec![0; IMAGE_PLAN_MAX_BYTES + 1]),
        Err(Error::PlanLength)
    ));
}

#[test]
fn repaired_outer_digest_cannot_authorize_foreign_addresses_or_bad_nested_pages() {
    let base = model(&[("items", DataType::Integer)]);
    let original = plan(&base, &[(1, "value")]).encode().unwrap();
    for address in [
        PageAddress::history([9; 16], 1).unwrap(),
        PageAddress::primary([7; 16], 1, 1).unwrap(),
        PageAddress::history([7; 16], 2).unwrap(),
    ] {
        let mut bytes = original.clone();
        bytes[192..256].copy_from_slice(&address.encode().unwrap());
        repair(&mut bytes);
        assert!(ImagePlan::decode(&bytes).is_err());
    }
    let index_address = root_offset(&original) + ROOT_BYTES + 8;
    for address in [
        PageAddress::primary([7; 16], 2, 1).unwrap(),
        PageAddress::primary([9; 16], 1, 1).unwrap(),
        PageAddress::history([7; 16], 1).unwrap(),
    ] {
        let mut bytes = original.clone();
        bytes[index_address..index_address + 64].copy_from_slice(&address.encode().unwrap());
        repair(&mut bytes);
        assert!(matches!(
            ImagePlan::decode(&bytes),
            Err(Error::Plan("envelope page scope"))
        ));
    }
    for offset in [256 + 100, index_address + 64 + 100] {
        let mut bytes = original.clone();
        bytes[offset] ^= 1;
        repair(&mut bytes);
        assert!(ImagePlan::decode(&bytes).is_err());
    }
}

#[test]
fn nested_counts_must_match_both_header_and_available_records_before_materialization() {
    let base = model(&[("items", DataType::Integer)]);
    let original = plan(&base, &[(1, "value")]).encode().unwrap();
    let offset = root_offset(&original) + ROOT_BYTES;
    for value in [0u32, 2, 1025, u32::MAX] {
        for field in [offset, offset + 4] {
            if original[field..field + 4] == value.to_le_bytes() {
                continue;
            }
            let mut bytes = original.clone();
            bytes[field..field + 4].copy_from_slice(&value.to_le_bytes());
            repair(&mut bytes);
            assert!(
                ImagePlan::decode(&bytes).is_err(),
                "field {field}, value {value}"
            );
        }
    }
    let mut wrong_total = original.clone();
    wrong_total[124..128].copy_from_slice(&2u32.to_le_bytes());
    repair(&mut wrong_total);
    assert!(matches!(
        ImagePlan::decode(&wrong_total),
        Err(Error::PlanLength)
    ));
}

#[test]
fn public_hash_and_valid_metadata_still_require_exact_base_and_complete_next_state() {
    let base = model(&[("items", DataType::Integer)]);
    let original = plan(&base, &[(1, "value")]).encode().unwrap();
    for offset in [48, 80] {
        let mut bytes = original.clone();
        bytes[offset] ^= 1;
        repair(&mut bytes);
        let decoded = ImagePlan::decode(&bytes).unwrap();
        assert!(decoded.replay(&base).is_err());
    }
    let other = Model::new([9; 16]).unwrap();
    assert!(matches!(
        ImagePlan::decode(&original).unwrap().replay(&other),
        Err(Error::Conflict)
    ));
    let staged = other.begin().unwrap();
    assert!(matches!(staged.prepare(), Err(Error::Empty)));
    // A decoded envelope supplies no publication authorization.
    let before = other.fingerprint();
    assert!(
        ImagePlan::decode(&original)
            .unwrap()
            .replay(&other)
            .is_err()
    );
    assert_eq!(other.fingerprint(), before);
}

#[test]
fn zero_image_rebuild_has_a_frozen_standalone_version_one_fixture() {
    let base = model(&[("items", DataType::Integer)]);
    let mut staged = base.begin().unwrap();
    staged.rebuild_index("items").unwrap();
    let original = staged.prepare().unwrap().image_plan().unwrap();
    let bytes = original.encode().unwrap();
    assert_eq!(bytes.len(), 424);
    assert_eq!(original.counts().unwrap().image_body_bytes(), 0);
    let frozen = include_str!("fixtures/empty-rebuild-ebip-1.hex");
    let fixture: Vec<u8> = frozen
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect();
    assert_eq!(bytes, fixture);
    let decoded = ImagePlan::decode(&fixture).unwrap();
    assert_eq!(
        decoded.replay(&base).unwrap().fingerprint(),
        original.next_fingerprint()
    );
}

#[test]
fn dropped_and_recreated_table_names_keep_distinct_encoded_root_scopes() {
    let base = model(&[("items", DataType::Integer), ("spare", DataType::Integer)]);
    let mut staged = base.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    staged
        .apply(Event {
            table_id: 3,
            kind: EventKind::Create(schema("items", DataType::Integer)),
        })
        .unwrap();
    staged.rebuild_index("items").unwrap();
    let original = staged.prepare().unwrap().image_plan().unwrap();
    let bytes = original.encode().unwrap();
    let decoded = ImagePlan::decode(&bytes).unwrap();
    assert_eq!(decoded.retired_tables()[0].binding().address().table(), 1);
    assert_eq!(decoded.roots()[0].binding().address().table(), 3);
    let replayed = decoded.replay(&base).unwrap();
    assert_eq!(replayed.view().table_id("items").unwrap(), 3);
    assert_eq!(
        replayed.selection(2).unwrap().binding(),
        base.selection(2).unwrap().binding()
    );
    assert_eq!(base.view().table_id("items").unwrap(), 1);
}

#[test]
fn nonfirst_text_primary_keys_preserve_utf8_nuls_and_3072_byte_exclusions() {
    let base = Model::new([7; 16]).unwrap();
    let mut staged = base.begin().unwrap();
    staged
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(Schema {
                name: "items".into(),
                primary_key: 1,
                columns: vec![
                    Column {
                        name: "value".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                    Column {
                        name: "id".into(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            }),
        })
        .unwrap();
    let keys = ["λ\0".into(), "λ".repeat(128), "λ".repeat(1536)];
    for key in &keys {
        staged
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![
                    Value::Text("private synthetic".into()),
                    Value::Text(key.clone()),
                ]),
            })
            .unwrap();
    }
    staged.rebuild_index("items").unwrap();
    let original = staged.prepare().unwrap().image_plan().unwrap();
    let decoded = ImagePlan::decode(&original.encode().unwrap()).unwrap();
    let replayed = decoded.replay(&base).unwrap();
    let binding = replayed.selection(1).unwrap().binding();
    assert_eq!((binding.covered(), binding.excluded()), (2, 1));
    for key in keys {
        assert_eq!(
            replayed
                .view()
                .get("items", &Key::Text(key))
                .unwrap()
                .unwrap()[0],
            Value::Text("private synthetic".into())
        );
    }
}

#[test]
fn all_128_created_and_retired_roots_round_trip_with_checked_combined_lengths() {
    let mut base = Model::new([7; 16]).unwrap();
    let mut staged = base.begin().unwrap();
    for id in 1..=128 {
        let name = format!("table_{id}");
        staged
            .apply(Event {
                table_id: id,
                kind: EventKind::Create(schema(&name, DataType::Integer)),
            })
            .unwrap();
        staged.rebuild_index(&name).unwrap();
    }
    let prepared = staged.prepare().unwrap();
    let plan = prepared.image_plan().unwrap();
    let bytes = plan.encode().unwrap();
    assert_eq!(plan.counts().unwrap().changed_roots(), 128);
    assert_eq!(
        plan.counts().unwrap().envelope_bytes().unwrap(),
        bytes.len() as u64
    );
    assert_eq!(
        ImagePlan::decode(&bytes)
            .unwrap()
            .replay(&base)
            .unwrap()
            .fingerprint(),
        plan.next_fingerprint()
    );
    base.publish(prepared).unwrap();
    let mut staged = base.begin().unwrap();
    for id in 1..=128 {
        staged
            .apply(Event {
                table_id: id,
                kind: EventKind::Drop,
            })
            .unwrap();
    }
    let plan = staged.prepare().unwrap().image_plan().unwrap();
    let decoded = ImagePlan::decode(&plan.encode().unwrap()).unwrap();
    assert_eq!(decoded.counts().unwrap().retired_tables(), 128);
    assert!(decoded.replay(&base).unwrap().view().schemas().is_empty());
    assert_eq!(base.view().schemas().len(), 128);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn decoded_accepted_and_discarded_updates_match_an_independent_row_map(
        commands in prop::collection::vec((0i64..12, 0u8..3, any::<u16>(), any::<bool>()), 1..24)
    ) {
        let mut base = model(&[("items", DataType::Integer)]);
        let mut expected = BTreeMap::new();
        for (key, operation, value, publish) in commands {
            let before = base.fingerprint();
            let old = base.clone();
            let text = value.to_string();
            let mut staged = base.begin().unwrap();
            let kind = match operation {
                0 => EventKind::Insert(vec![Value::Integer(key), Value::Text(text.clone())]),
                1 => EventKind::Replace(vec![Value::Integer(key), Value::Text(text.clone())]),
                _ => EventKind::Delete(Key::Integer(key)),
            };
            let succeeds = if operation == 0 { !expected.contains_key(&key) } else { expected.contains_key(&key) };
            prop_assert_eq!(staged.apply(Event { table_id: 1, kind }).is_ok(), succeeds);
            if !succeeds { continue; }
            staged.rebuild_index("items").unwrap();
            let prepared = staged.prepare().unwrap();
            let original = prepared.image_plan().unwrap();
            let bytes = original.encode().unwrap();
            let decoded = ImagePlan::decode(&bytes).unwrap();
            prop_assert_eq!(decoded.encode().unwrap(), bytes);
            let replayed = decoded.replay(&base).unwrap();
            let mut candidate = expected.clone();
            if operation == 2 { candidate.remove(&key); } else { candidate.insert(key, text); }
            let rows: Vec<_> = candidate.iter().map(|(key,value)| vec![Value::Integer(*key), Value::Text(value.clone())]).collect();
            prop_assert_eq!(replayed.view().scan("items", 100).unwrap(), rows);
            prop_assert_eq!(base.fingerprint(), before);
            if publish { base.publish(prepared).unwrap(); expected = candidate; prop_assert_eq!(base.fingerprint(), replayed.fingerprint()); }
            prop_assert_eq!(old.fingerprint(), before);
        }
    }

    #[test]
    fn exact_envelope_arithmetic_matches_an_independent_sum(
        history in 0usize..=256, primary in 0usize..=2048, retired in 0usize..=2048,
        roots in 1usize..=128, tables in 0usize..=128
    ) {
        let counts = PlanCounts::from_counts(history, primary, retired, roots, tables).unwrap();
        let expected = 192u64 + 32 + (history as u64 + primary as u64) * 4160
            + roots as u64 * 200 + retired as u64 * 64 + tables as u64 * 224;
        prop_assert_eq!(counts.envelope_bytes().unwrap(), expected);
        prop_assert!(expected <= IMAGE_PLAN_MAX_BYTES as u64);
    }
}

#[test]
fn recomputed_digest_cannot_retire_an_upserted_image_or_reorder_root_scopes() {
    let base = model(&[("items", DataType::Integer)]);
    let original = plan(&base, &[(1, "value")]).encode().unwrap();
    let root = root_offset(&original);
    let image_address = root + ROOT_BYTES + 8;
    let mut overlap = original.clone();
    let address = overlap[image_address..image_address + 64].to_vec();
    let end = overlap.len() - 32;
    overlap.splice(end..end, address);
    overlap[root + ROOT_BYTES + 4..root + ROOT_BYTES + 8].copy_from_slice(&1u32.to_le_bytes());
    overlap[128..132].copy_from_slice(&1u32.to_le_bytes());
    let length = overlap.len() as u64;
    overlap[132..140].copy_from_slice(&length.to_le_bytes());
    repair(&mut overlap);
    assert!(matches!(
        ImagePlan::decode(&overlap),
        Err(Error::Plan("envelope index retirement order/overlap"))
    ));

    let base = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let mut staged = base.begin().unwrap();
    for (id, name) in [(1, "left"), (2, "right")] {
        staged
            .apply(Event {
                table_id: id,
                kind: EventKind::Insert(vec![Value::Integer(1), Value::Text(name.into())]),
            })
            .unwrap();
        staged.rebuild_index(name).unwrap();
    }
    let original = staged
        .prepare()
        .unwrap()
        .image_plan()
        .unwrap()
        .encode()
        .unwrap();
    let root = root_offset(&original);
    let width = 192 + 8 + 64 + 4096;
    let mut swapped = original.clone();
    swapped[root..root + width].copy_from_slice(&original[root + width..root + 2 * width]);
    swapped[root + width..root + 2 * width].copy_from_slice(&original[root..root + width]);
    repair(&mut swapped);
    assert!(matches!(
        ImagePlan::decode(&swapped),
        Err(Error::Plan("envelope changed root order/predecessor"))
    ));
    let mut duplicate = original.clone();
    duplicate[root + width..root + 2 * width].copy_from_slice(&original[root..root + width]);
    repair(&mut duplicate);
    assert!(ImagePlan::decode(&duplicate).is_err());
}

#[test]
fn duplicate_gap_and_reordered_retired_roots_are_rejected_after_digest_repair() {
    let base = model(&[("items", DataType::Integer)]);
    let value = "x".repeat(3072);
    let original = plan(&base, &[(0, &value), (1, &value), (2, &value)])
        .encode()
        .unwrap();
    let history = u32::from_le_bytes(original[112..116].try_into().unwrap());
    assert_eq!(history, 3);
    let first = PageAddress::decode(&original[192..256]).unwrap();
    for id in [first.page(), first.page() + 2] {
        let mut bytes = original.clone();
        bytes[4352..4416]
            .copy_from_slice(&PageAddress::history([7; 16], id).unwrap().encode().unwrap());
        repair(&mut bytes);
        assert!(matches!(
            ImagePlan::decode(&bytes),
            Err(Error::Plan("envelope history continuity"))
        ));
    }
    let base = model(&[("left", DataType::Integer), ("right", DataType::Integer)]);
    let mut staged = base.begin().unwrap();
    for id in [1, 2] {
        staged
            .apply(Event {
                table_id: id,
                kind: EventKind::Drop,
            })
            .unwrap();
    }
    let original = staged
        .prepare()
        .unwrap()
        .image_plan()
        .unwrap()
        .encode()
        .unwrap();
    let offset = root_offset(&original);
    let mut bytes = original.clone();
    bytes[offset..offset + 224].copy_from_slice(&original[offset + 224..offset + 448]);
    bytes[offset + 224..offset + 448].copy_from_slice(&original[offset..offset + 224]);
    repair(&mut bytes);
    assert!(matches!(
        ImagePlan::decode(&bytes),
        Err(Error::Plan("envelope retired root order/scope"))
    ));
}

#[test]
fn exact_independent_maximum_includes_every_envelope_component() {
    let counts = PlanCounts::from_counts(256, 2048, 2048, 128, 128).unwrap();
    assert_eq!(counts.envelope_bytes().unwrap(), 9_770_208);
    assert_eq!(
        counts.envelope_bytes().unwrap(),
        IMAGE_PLAN_MAX_BYTES as u64
    );
    assert_eq!(
        counts.envelope_bytes().unwrap() - counts.image_body_bytes(),
        333_024
    );
    // These are loose independent bounds. Not every simultaneous maximum is
    // reachable in one live table state, and none is an allocator/RSS bound.
}
