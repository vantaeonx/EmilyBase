use super::*;
use emilybase_catalog::{Column, DataType, Key, Schema, Value};
use emilybase_database::{Event, EventKind};

fn schema(name: &str) -> Schema {
    Schema {
        name: name.into(),
        primary_key: 0,
        columns: vec![
            Column {
                name: "id".into(),
                data_type: DataType::Integer,
                nullable: false,
            },
            Column {
                name: "value".into(),
                data_type: DataType::Text,
                nullable: false,
            },
        ],
    }
}

fn prepared() -> (Model, Prepared) {
    let mut base = Model::new([7; 16]).unwrap();
    let mut stage = base.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Create(schema("items")),
        })
        .unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Insert(vec![Value::Integer(1), Value::Text("old".into())]),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    base.publish(stage.prepare().unwrap()).unwrap();
    let mut stage = base.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Replace(vec![Value::Integer(1), Value::Text("new".into())]),
        })
        .unwrap();
    stage.rebuild_index("items").unwrap();
    (base, stage.prepare().unwrap())
}

#[test]
fn every_single_byte_image_damage_refuses_without_changing_base() {
    let (base, prepared) = prepared();
    let mut plan = prepared.image_plan().unwrap();
    let before = base.fingerprint();
    for offset in 0..PAGE_SIZE {
        plan.history[0].image[offset] ^= 1;
        assert!(plan.replay(&base).is_err(), "history offset {offset}");
        plan.history[0].image[offset] ^= 1;
        plan.roots[0].upserts[0].image[offset] ^= 1;
        assert!(plan.replay(&base).is_err(), "index offset {offset}");
        plan.roots[0].upserts[0].image[offset] ^= 1;
        assert_eq!(base.fingerprint(), before);
    }
    assert_eq!(
        plan.replay(&base).unwrap().fingerprint(),
        plan.next_fingerprint()
    );
}

#[test]
fn repaired_page_crc_cannot_rewrite_an_acknowledged_slot() {
    let (base, prepared) = prepared();
    let mut plan = prepared.image_plan().unwrap();
    let write = &mut plan.history[0];
    let mut page = Page::decode(&write.image, write.address.page()).unwrap();
    page.update(0, b"forged marker").unwrap();
    write.image = page.encode();
    assert!(matches!(
        plan.replay(&base),
        Err(Error::Plan("committed history rewrite"))
    ));
    assert_eq!(
        base.view().get("items", &Key::Integer(1)).unwrap().unwrap()[1],
        Value::Text("old".into())
    );
}

#[test]
fn equal_page_numbers_do_not_authorize_foreign_domains_tables_or_databases() {
    let (base, prepared) = prepared();
    for fault in 0..6 {
        let mut plan = prepared.image_plan().unwrap();
        match fault {
            0 => plan.history[0].address = PageAddress::primary([7; 16], 1, 1).unwrap(),
            1 => plan.history[0].address = PageAddress::history([8; 16], 1).unwrap(),
            2 => plan.roots[0].upserts[0].address = PageAddress::history([7; 16], 1).unwrap(),
            3 => plan.roots[0].upserts[0].address = PageAddress::primary([7; 16], 2, 1).unwrap(),
            4 => plan.roots[0].upserts[0].address = PageAddress::primary([8; 16], 1, 1).unwrap(),
            _ => plan.roots[0].upserts[0].address = PageAddress::primary([7; 16], 1, 2).unwrap(),
        }
        assert!(plan.replay(&base).is_err());
    }
}

#[test]
fn exact_base_adjacent_transaction_and_expected_next_fingerprint_are_mandatory() {
    let (base, prepared) = prepared();
    for fault in 0..5 {
        let mut plan = prepared.image_plan().unwrap();
        match fault {
            0 => plan.base[0] ^= 1,
            1 => plan.database[0] ^= 1,
            2 => plan.base_transaction += 1,
            3 => plan.transaction += 1,
            _ => plan.next[0] ^= 1,
        }
        assert!(plan.replay(&base).is_err());
    }
}

#[test]
fn omitted_duplicate_and_unchanged_records_cannot_select_a_partial_state() {
    let (base, prepared) = prepared();
    for fault in 0..6 {
        let mut plan = prepared.image_plan().unwrap();
        match fault {
            0 => plan.history.clear(),
            1 => plan.roots.clear(),
            2 => plan.roots[0].upserts.clear(),
            3 => {
                let duplicate = PageWrite {
                    address: plan.roots[0].upserts[0].address,
                    image: plan.roots[0].upserts[0].image,
                };
                plan.roots[0].upserts.push(duplicate);
            }
            4 => plan.history.push(PageWrite {
                address: plan.history[0].address,
                image: plan.history[0].image,
            }),
            _ => plan.history[0].image = base.view().pages().last().unwrap().encode(),
        }
        assert!(plan.replay(&base).is_err(), "fault {fault}");
    }
}

#[test]
fn valid_index_crc_and_topology_cannot_select_another_tables_row_pointer() {
    let base = Model::new([7; 16]).unwrap();
    let mut stage = base.begin().unwrap();
    for (table, name) in [(1, "left"), (2, "right")] {
        stage
            .apply(Event {
                table_id: table,
                kind: EventKind::Create(schema(name)),
            })
            .unwrap();
        stage
            .apply(Event {
                table_id: table,
                kind: EventKind::Insert(vec![Value::Integer(1), Value::Text(name.into())]),
            })
            .unwrap();
        stage.rebuild_index(name).unwrap();
    }
    let prepared = stage.prepare().unwrap();
    let mut plan = prepared.image_plan().unwrap();
    let foreign = plan.roots[1].upserts[0].image;
    plan.roots[0].upserts[0].image = foreign;
    assert!(plan.replay(&base).is_err());
    assert_eq!(base.view().row_count(), 0);
}

#[test]
fn unknown_wrong_domain_duplicate_and_overlap_retirement_is_refused() {
    let (base, prepared) = prepared();
    for fault in 0..5 {
        let mut plan = prepared.image_plan().unwrap();
        let address = match fault {
            0 => PageAddress::history([7; 16], 1).unwrap(),
            1 => PageAddress::primary([7; 16], 2, 1).unwrap(),
            2 => PageAddress::primary([7; 16], 1, 99).unwrap(),
            3 => plan.roots[0].upserts[0].address,
            _ => PageAddress::primary([7; 16], 1, 99).unwrap(),
        };
        plan.roots[0].retired.push(address);
        if fault == 4 {
            plan.roots[0].retired.push(address);
        }
        assert!(plan.replay(&base).is_err());
    }
}

#[test]
fn valid_crc_cannot_rewrite_an_earlier_history_page_or_introduce_a_gap() {
    let (mut base, _) = prepared();
    let mut stage = base.begin().unwrap();
    for key in 2..5 {
        stage
            .apply(Event {
                table_id: 1,
                kind: EventKind::Insert(vec![Value::Integer(key), Value::Text("x".repeat(3072))]),
            })
            .unwrap();
    }
    stage.rebuild_index("items").unwrap();
    base.publish(stage.prepare().unwrap()).unwrap();
    assert!(base.view().page_count() > 1);
    let mut stage = base.begin().unwrap();
    let event = Event {
        table_id: 1,
        kind: EventKind::Replace(vec![Value::Integer(1), Value::Text("next".into())]),
    };
    stage.apply(event.clone()).unwrap();
    stage.rebuild_index("items").unwrap();
    let prepared = stage.prepare().unwrap();
    for earlier in [true, false] {
        let mut plan = prepared.image_plan().unwrap();
        let id = if earlier {
            1
        } else {
            base.view().page_count() as u64 + 2
        };
        let mut page = if earlier {
            base.view().pages().next().unwrap().clone()
        } else {
            Page::new(id).unwrap()
        };
        page.insert(&event.encode().unwrap()).unwrap();
        plan.history[0] = PageWrite {
            address: PageAddress::history(base.database_id(), id).unwrap(),
            image: page.encode(),
        };
        assert!(matches!(
            plan.replay(&base),
            Err(Error::Plan("history page gap or earlier rewrite"))
        ));
    }
}

#[test]
fn table_retirement_requires_the_exact_old_binding_and_image_fingerprint() {
    let (base, _) = prepared();
    let mut stage = base.begin().unwrap();
    stage
        .apply(Event {
            table_id: 1,
            kind: EventKind::Drop,
        })
        .unwrap();
    let prepared = stage.prepare().unwrap();
    let mut plan = prepared.image_plan().unwrap();
    assert!(plan.replay(&base).is_ok());
    plan.retired[0].fingerprint[0] ^= 1;
    assert!(matches!(
        plan.replay(&base),
        Err(Error::Plan("retired root predecessor"))
    ));
    plan.retired[0].fingerprint[0] ^= 1;
    plan.retired.push(RetiredRoot {
        binding: plan.retired[0].binding,
        fingerprint: plan.retired[0].fingerprint,
    });
    assert!(matches!(
        plan.replay(&base),
        Err(Error::Plan("retired table order"))
    ));
}
