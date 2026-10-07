#![no_main]
#![forbid(unsafe_code)]
use emilybase_auth::accounts::inspect_private_account_backup_bytes;
use emilybase_catalog::{Column, DataType, Schema, Value};
use emilybase_database::{Event, EventKind, Snapshot};
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;

const PROJECT: &str = "11111111111111111111111111111111";
fn schema(name: &str, columns: &[(&str, DataType)]) -> Schema {
    Schema {
        name: name.into(),
        primary_key: 0,
        columns: columns
            .iter()
            .map(|(name, ty)| Column {
                name: (*name).into(),
                data_type: *ty,
                nullable: false,
            })
            .collect(),
    }
}
fn verifier() -> Vec<u8> {
    // Synthetic opaque verifier payload: format inspection, never password proof.
    let mut bytes = vec![0; 72];
    bytes[..8].copy_from_slice(b"EBPWD\0\0\0");
    bytes[8..12].copy_from_slice(&[1, 0, 2, 19]);
    bytes[12..16].copy_from_slice(&19456_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&2_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes
}
fn event(snapshot: &mut Snapshot, table_id: u64, kind: EventKind) {
    snapshot.apply(Event { table_id, kind }).unwrap();
}
fuzz_target!(|bytes: &[u8]| {
    // Raw inputs reach complete header/WAL/private inventory validation.
    let _ = inspect_private_account_backup_bytes(bytes, PROJECT);
    if bytes.is_empty() || bytes.len() > 256 {
        return;
    }
    let mut image = Snapshot::empty().unwrap();
    event(
        &mut image,
        1,
        EventKind::Create(schema(
            "auth_scope",
            &[
                ("id", DataType::Integer),
                ("version", DataType::Integer),
                ("project", DataType::Text),
                ("dummy", DataType::Bytes),
            ],
        )),
    );
    event(
        &mut image,
        2,
        EventKind::Create(schema(
            "auth_users",
            &[
                ("login", DataType::Text),
                ("id", DataType::Bytes),
                ("digest", DataType::Bytes),
                ("epoch", DataType::Integer),
                ("disabled", DataType::Boolean),
            ],
        )),
    );
    let version = i64::from(bytes[0] % 5);
    let wrong_project = bytes[0] & 8 != 0;
    let bad_dummy = bytes[0] & 16 != 0;
    let mut dummy = verifier();
    if bad_dummy {
        dummy[10] = 3;
    }
    event(
        &mut image,
        1,
        EventKind::Insert(vec![
            Value::Integer(1),
            Value::Integer(version),
            Value::Text(
                if wrong_project {
                    "22222222222222222222222222222222"
                } else {
                    PROJECT
                }
                .into(),
            ),
            Value::Bytes(dummy),
        ]),
    );
    let mut valid = version == 1 && !wrong_project && !bad_dummy;
    let mut identities = BTreeSet::new();
    let mut count = 0;
    for (index, command) in bytes[1..].as_chunks::<4>().0.iter().take(8).enumerate() {
        let id = vec![command[1]; if command[0] & 1 == 0 { 16 } else { 15 }];
        let epoch = match command[2] % 4 {
            0 => 0,
            1 => -1,
            2 => i64::MAX,
            _ => 1,
        };
        let login = if command[0] & 2 == 0 {
            format!("u{index}")
        } else {
            format!("U{index}")
        };
        let mut digest = verifier();
        if command[0] & 4 != 0 {
            digest[8] = 2;
        }
        valid &=
            id.len() == 16 && epoch > 0 && command[0] & 6 == 0 && identities.insert(id.clone());
        event(
            &mut image,
            2,
            EventKind::Insert(vec![
                Value::Text(login),
                Value::Bytes(id),
                Value::Bytes(digest),
                Value::Integer(epoch),
                Value::Boolean(command[3] & 1 != 0),
            ]),
        );
        count += 1;
    }
    let pages = image.pages().cloned().collect::<Vec<_>>();
    let wal = emilybase_wal::encode_snapshot([7; 16], 1, &pages).unwrap();
    let archive = emilybase_backup::encode(&wal).unwrap();
    let inspected = inspect_private_account_backup_bytes(&archive, PROJECT);
    assert_eq!(inspected.is_ok(), valid);
    if let Ok(report) = inspected {
        assert_eq!(report.accounts, count);
        assert_eq!(report.private_version, 1);
        assert_eq!(report.session_families, 0);
        assert_eq!(report.clock_floor, None);
    }
});
