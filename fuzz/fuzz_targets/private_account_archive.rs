#![no_main]
#![forbid(unsafe_code)]
use emilybase_auth::accounts::inspect_private_account_backup_bytes;
use emilybase_auth::row_policy::{TableContext, records};
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
    let version = i64::from(bytes[0] % 6);
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
    let mut valid = matches!(version, 1 | 4 | 5) && !wrong_project && !bad_dummy;
    if version >= 4 {
        event(
            &mut image,
            3,
            EventKind::Create(schema(
                "auth_sessions_meta",
                &[
                    ("id", DataType::Integer),
                    ("version", DataType::Integer),
                    ("incarnation", DataType::Bytes),
                ],
            )),
        );
        event(
            &mut image,
            3,
            EventKind::Insert(vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Bytes(vec![2; 16]),
            ]),
        );
        event(
            &mut image,
            4,
            EventKind::Create(schema(
                "auth_sessions",
                &[
                    ("family", DataType::Text),
                    ("incarnation", DataType::Bytes),
                    ("login", DataType::Text),
                    ("user", DataType::Bytes),
                    ("epoch", DataType::Integer),
                    ("generation", DataType::Integer),
                    ("created", DataType::Integer),
                    ("issued", DataType::Integer),
                    ("access_until", DataType::Integer),
                    ("refresh_until", DataType::Integer),
                    ("absolute_until", DataType::Integer),
                    ("access", DataType::Bytes),
                    ("refresh", DataType::Bytes),
                    ("revoked", DataType::Boolean),
                ],
            )),
        );
        event(
            &mut image,
            5,
            EventKind::Create(schema(
                "auth_session_clock",
                &[
                    ("id", DataType::Integer),
                    ("version", DataType::Integer),
                    ("observed", DataType::Integer),
                ],
            )),
        );
        event(
            &mut image,
            5,
            EventKind::Insert(vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(100),
            ]),
        );
        event(&mut image, 6, EventKind::Create(records::header_schema()));
        event(&mut image, 7, EventKind::Create(records::chunk_schema()));
        let command = bytes.get(1).copied().unwrap_or(0);
        let defect = command % 5;
        let target = schema("items", &[("id", DataType::Integer)]);
        let mut document = br#"{"version":1,"select":{"kind":"deny"},"insert":{"kind":"deny"},"update_using":{"kind":"deny"},"update_check":{"kind":"deny"},"delete":{"kind":"deny"}}"#.to_vec();
        if command & 8 != 0 {
            document.resize(16_384, b' ');
        }
        let encoded = records::encode(
            TableContext {
                project: PROJECT,
                id: u64::MAX,
                schema: &target,
            },
            if defect == 2 { u64::MAX } else { 40 },
            0,
            &document,
        )
        .unwrap();
        let (header, mut chunks) = encoded.into_rows();
        event(&mut image, 6, EventKind::Insert(header));
        if defect == 3 {
            let Value::Bytes(payload) = &mut chunks[0][1] else {
                unreachable!()
            };
            payload[0] ^= 1;
        }
        for (index, row) in chunks.into_iter().enumerate() {
            if defect != 4 || index != 0 {
                event(&mut image, 7, EventKind::Insert(row));
            }
        }
        if defect == 1 {
            event(
                &mut image,
                7,
                EventKind::Insert(vec![Value::Text("7:0".into()), Value::Bytes(vec![0])]),
            );
        }
        valid &= defect == 0;
    }
    if version == 5 {
        event(
            &mut image,
            8,
            EventKind::Create(schema(
                "auth_public_admission",
                &[
                    ("id", DataType::Integer),
                    ("version", DataType::Integer),
                    ("enabled", DataType::Boolean),
                    ("revision", DataType::Text),
                    ("previous", DataType::Text),
                ],
            )),
        );
        let defect = bytes.get(2).copied().unwrap_or(0) % 8;
        let (enabled, revision, previous) = match defect {
            0 => (false, "41", "0"),
            1 => (true, "41", "40"),
            2 => (false, "0", "0"),
            3 => (false, "041", "0"),
            4 => (false, "18446744073709551615", "0"),
            5 => (false, "41", "41"),
            6 => (true, "41", "0"),
            _ => (false, "41", "0"),
        };
        if defect != 7 {
            event(
                &mut image,
                8,
                EventKind::Insert(vec![
                    Value::Integer(1),
                    Value::Integer(1),
                    Value::Boolean(enabled),
                    Value::Text(revision.into()),
                    Value::Text(previous.into()),
                ]),
            );
        }
        valid &= defect <= 1;
    }
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
    let wal = emilybase_wal::encode_snapshot([7; 16], 42, &pages).unwrap();
    let archive = emilybase_backup::encode(&wal).unwrap();
    let inspected = inspect_private_account_backup_bytes(&archive, PROJECT);
    assert_eq!(inspected.is_ok(), valid);
    if let Ok(report) = inspected {
        assert_eq!(report.accounts, count);
        assert_eq!(report.private_version, version as u16);
        assert_eq!(report.session_families, 0);
        assert_eq!(
            report.clock_floor,
            if version >= 4 { Some(100) } else { None }
        );
    }
});
