use super::*;
use emilybase_catalog::Row;
use proptest::prelude::*;

fn record(number: u8) -> records::Record {
    let mut record = tests::fixture_record(
        &format!("a{number:03}"),
        [number + 1; 16],
        u64::from(number) + 1,
    );
    record.info.disabled = number.is_multiple_of(3);
    record
}
fn opened(path: &Path, records: Vec<Row>, version: u16, compact: bool) -> AccountStore {
    tests::raw_store(path, records);
    let mut store =
        AccountStore::open(path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
    if version == 2 {
        store.enable_session_storage().unwrap();
    }
    if version == 3 {
        store.enable_session_clock(50).unwrap();
    }
    if compact {
        store.compact().unwrap();
    }
    store
}

#[test]
fn bounded_metadata_pages_cover_every_private_version_and_wal_without_writes_or_hash_work() {
    let _io = TEST_IO.lock().unwrap();
    for version in [1, 2, 3] {
        for compact in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic");
            let mut records: Vec<_> = (0..129).rev().map(record).collect();
            records.push(tests::fixture_record(
                &"z".repeat(64),
                [250; 16],
                i64::MAX as u64,
            ));
            let mut expected: Vec<_> = records.iter().map(|r| r.info.clone()).collect();
            expected.sort_by(|a, b| a.login.cmp(&b.login));
            let rows = records.into_iter().map(|r| r.encode()).collect();
            let mut store = opened(&path, rows, version, compact);
            let before = store.database.committed_wal().unwrap();
            let clock = store.session_clock_floor().unwrap();
            for limit in [1, 2, 7, MAX_ACCOUNT_PAGE] {
                let mut after = None;
                let mut actual = Vec::new();
                loop {
                    let page = store.list_users(after.as_deref(), limit).unwrap();
                    assert!(page.users.len() <= limit);
                    assert_eq!(format!("{page:?}"), "AccountPage(redacted)");
                    if page.next_after.is_some() {
                        assert_eq!(
                            page.next_after.as_deref(),
                            page.users.last().map(|r| r.login.as_str())
                        );
                    }
                    actual.extend(page.users);
                    after = page.next_after;
                    assert_eq!(store.database.committed_wal().unwrap(), before);
                    assert_eq!(store.session_clock_floor().unwrap(), clock);
                    assert_eq!(store.pool.usage().operations, 0);
                    if after.is_none() {
                        break;
                    }
                }
                assert_eq!(actual, expected);
            }
            let gap = store.list_users(Some("a003x"), 2).unwrap();
            assert_eq!(
                gap.users
                    .iter()
                    .map(|r| r.login.as_str())
                    .collect::<Vec<_>>(),
                vec!["a004", "a005"]
            );
            assert_eq!(gap.next_after.as_deref(), Some("a005"));
            let end = store.list_users(Some(&"z".repeat(64)), 128).unwrap();
            assert!(end.users.is_empty() && end.next_after.is_none());
            drop(store);
            let mut store =
                AccountStore::open(&path, tests::PROJECT, PasswordPool::new(1).unwrap()).unwrap();
            let first = store.list_users(None, 128).unwrap();
            assert_eq!(first.users, expected[..128]);
            assert_eq!(first.next_after.as_deref(), Some("a127"));
            let last = store.list_users(first.next_after.as_deref(), 128).unwrap();
            assert_eq!(last.users, expected[128..]);
            assert!(last.next_after.is_none());
            assert_eq!(store.database.committed_wal().unwrap(), before);
        }
    }
}

#[test]
fn invalid_page_limits_and_noncanonical_cursors_refuse_without_mutating_state() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = opened(
        &dir.path().join("synthetic"),
        vec![record(1).encode()],
        3,
        false,
    );
    let before = store.database.committed_wal().unwrap();
    for limit in [0, MAX_ACCOUNT_PAGE + 1, usize::MAX] {
        assert!(matches!(store.list_users(None, limit), Err(Error::Page)));
    }
    for after in [
        "",
        "A",
        "../escape",
        "a/b",
        "a\\b",
        "_private",
        "a\0b",
        "a b",
        "имя",
        &"a".repeat(65),
    ] {
        assert!(matches!(
            store.list_users(Some(after), 1),
            Err(Error::Login)
        ));
    }
    let end = store.list_users(Some("zzzz"), 128).unwrap();
    assert!(end.users.is_empty() && end.next_after.is_none());
    assert_eq!(store.database.committed_wal().unwrap(), before);
    assert_eq!(store.session_clock_floor().unwrap(), Some(50));
}

#[test]
fn corrupt_consumed_record_and_lookahead_refuse_without_returning_partial_metadata() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = opened(
        &dir.path().join("synthetic"),
        vec![record(1).encode(), record(2).encode()],
        1,
        false,
    );
    let mut row = record(2).encode();
    row[2] = Value::Bytes(vec![0; 72]);
    let mut tx = store.database.begin().unwrap();
    tx.update(USERS, &Key::Text("a002".into()), row).unwrap();
    tx.commit().unwrap();
    let before = store.database.committed_wal().unwrap();
    for (after, limit) in [(None, 1), (None, 128), (Some("a001"), 1)] {
        assert!(matches!(
            store.list_users(after, limit),
            Err(Error::Corrupt)
        ));
    }
    assert!(matches!(store.list_users(None, 0), Err(Error::Page)));
    assert_eq!(store.database.committed_wal().unwrap(), before);
}

#[test]
fn continuation_reads_current_state_without_claiming_a_cross_request_snapshot() {
    let _io = TEST_IO.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let rows = ["a", "c", "e"]
        .into_iter()
        .enumerate()
        .map(|(i, login)| tests::fixture_record(login, [(i + 1) as u8; 16], 1).encode())
        .collect();
    let mut store = opened(&dir.path().join("synthetic"), rows, 3, false);
    let first = store.list_users(None, 2).unwrap();
    assert_eq!(first.next_after.as_deref(), Some("c"));
    store.create_user("b", b"synthetic-new-password").unwrap();
    store.create_user("d", b"synthetic-new-password").unwrap();
    store.set_disabled("c", true).unwrap();
    let before = store.database.committed_wal().unwrap();
    let next = store.list_users(first.next_after.as_deref(), 2).unwrap();
    assert_eq!(
        next.users
            .iter()
            .map(|u| u.login.as_str())
            .collect::<Vec<_>>(),
        vec!["d", "e"]
    );
    assert!(next.next_after.is_none());
    let fresh = store.list_users(None, 3).unwrap();
    assert_eq!(fresh.users[2].login, "c");
    assert!(fresh.users[2].disabled);
    assert_eq!(fresh.users[2].credential_epoch, 2);
    assert_eq!(store.database.committed_wal().unwrap(), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn metadata_pagination_matches_independent_sorted_inventory(
        keys in prop::collection::btree_set(0..40_u8,0..24),
        boundary in prop::option::of(0..45_u8),
        limit in 1..=8_usize,
        compact in any::<bool>(),
    ) {
        let _io=TEST_IO.lock().unwrap();
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("synthetic");
        let expected: Vec<_> = keys.into_iter().map(record).map(|r|r.info).collect();
        let rows=expected.iter().map(|info|{
            let mut r=tests::fixture_record(&info.login,info.id,info.credential_epoch);
            r.info.disabled=info.disabled;
            r.encode()
        }).collect();
        let mut store=opened(&path,rows,3,compact);
        let before=store.database.committed_wal().unwrap();
        let after=boundary.map(|n|format!("a{n:03}"));
        let wanted: Vec<_>=expected.into_iter().filter(|u|after.as_ref().is_none_or(|after|u.login>*after)).collect();
        let mut cursor=after;
        let mut actual=Vec::new();
        loop {
            let page=store.list_users(cursor.as_deref(),limit).unwrap();
            prop_assert!(page.users.len()<=limit);
            actual.extend(page.users);
            cursor=page.next_after;
            if cursor.is_none() {break;}
            prop_assert!(actual.len()<=wanted.len());
        }
        prop_assert_eq!(actual,wanted);
        prop_assert_eq!(store.database.committed_wal().unwrap(),before);
    }
}
