mod files;
mod root;
mod root_capture;
mod root_live;
use super::*;
use crate::{ProjectStore, durability};
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_catalog::Value;
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Fixture {
    registry: ProjectStore,
    accounts: Vec<AccountStore>,
    data_paths: Vec<PathBuf>,
    private_paths: Vec<PathBuf>,
    credentials: Vec<(String, String)>,
}
fn fixture(parent: &Path, count: usize) -> Fixture {
    let pool = PasswordPool::new(1).unwrap();
    let mut result = Fixture {
        registry: ProjectStore::open(parent.join("registry")).unwrap(),
        accounts: Vec::new(),
        data_paths: Vec::new(),
        private_paths: Vec::new(),
        credentials: Vec::new(),
    };
    for index in 0..count {
        let created = result.registry.create("synthetic project").unwrap();
        result
            .registry
            .authorize(&created.project.id, &created.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY,v INT); INSERT INTO t VALUES(1,$1)",
                &[Value::Integer(index as i64)],
            )
            .unwrap();
        let data_path = parent
            .join("registry")
            .join(&created.project.id)
            .join("data");
        let private_path = parent.join(format!("private-{index}"));
        let mut account =
            AccountStore::create(&private_path, &created.project.id, pool.clone()).unwrap();
        account
            .create_user("synthetic_user", b"synthetic-password")
            .unwrap();
        if index % 3 == 1 {
            account.enable_session_storage().unwrap();
        } else if index % 3 == 2 {
            account.enable_session_clock(100).unwrap();
            account
                .sign_in("synthetic_user", b"synthetic-password", 100)
                .unwrap();
        }
        if index % 2 == 1 {
            account.compact().unwrap();
            Database::open(&data_path).unwrap().compact().unwrap();
        }
        result.accounts.push(account);
        result.data_paths.push(data_path);
        result.private_paths.push(private_path);
        result
            .credentials
            .push((created.project.id, created.api_key));
    }
    result
}
fn histories(f: &Fixture) -> Vec<Vec<u8>> {
    f.data_paths
        .iter()
        .chain(&f.private_paths)
        .map(|p| std::fs::read(p.join("redo.wal")).unwrap())
        .collect()
}
fn seal(bytes: &mut [u8]) {
    let length = (bytes.len() - HEADER) as u64;
    bytes[24..32].copy_from_slice(&length.to_le_bytes());
    let hash = Sha256::digest(&bytes[HEADER..]);
    bytes[32..64].copy_from_slice(&hash);
    let crc = crc32fast::hash(&bytes[..124]);
    bytes[124..128].copy_from_slice(&crc.to_le_bytes());
}
fn parts(bytes: &[u8]) -> (&[u8], Vec<(String, Vec<u8>)>) {
    let length = u64_at(bytes, 16) as usize;
    let registry = &bytes[HEADER..HEADER + length];
    let mut at = HEADER + length;
    let mut entries = Vec::new();
    for _ in 0..u32_at(bytes, 12) {
        let length = u64_at(bytes, at + 32) as usize;
        let project = std::str::from_utf8(&bytes[at..at + 32]).unwrap().to_owned();
        at += ENTRY;
        entries.push((project, bytes[at..at + length].to_vec()));
        at += length;
    }
    assert_eq!(at, bytes.len());
    (registry, entries)
}
fn raw(registry: &[u8], entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = vec![0; HEADER];
    bytes[..8].copy_from_slice(b"EMILYBND");
    bytes[8..10].copy_from_slice(&1_u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&(registry.len() as u64).to_le_bytes());
    bytes.extend_from_slice(registry);
    for (project, archive) in entries {
        assert_eq!(project.len(), 32);
        bytes.extend_from_slice(project.as_bytes());
        bytes.extend_from_slice(&(archive.len() as u64).to_le_bytes());
        bytes.extend_from_slice(archive);
    }
    seal(&mut bytes);
    bytes
}

#[test]
fn common_capture_preserves_registry_and_private_versions_without_changing_source() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    let before = histories(&f);
    let registry = f.registry.backup_image().unwrap();
    let mut private = f
        .accounts
        .iter_mut()
        .map(|a| (a.project().to_owned(), a.backup_image().unwrap()))
        .collect::<Vec<_>>();
    private.sort_by(|a, b| a.0.cmp(&b.0));
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let report = inspect_account_bundle_bytes(&image).unwrap();
    assert_eq!(
        report.registry,
        crate::inspect_registry_backup_bytes(&registry).unwrap()
    );
    assert_eq!(parts(&image), (registry.as_slice(), private));
    assert_eq!(report.private_accounts.len(), 3);
    for entry in &report.private_accounts {
        let index = f
            .credentials
            .iter()
            .position(|(id, _)| id == &entry.project)
            .unwrap();
        assert_eq!(entry.inventory.private_version, index as u16 + 1);
        assert_eq!(
            entry.inventory.database.wal_version,
            if index == 1 { 2 } else { 1 }
        );
        assert_eq!(entry.inventory.accounts, 1);
        assert_eq!(entry.inventory.session_families, usize::from(index == 2));
        assert_eq!(
            entry.inventory.clock_floor,
            if index == 2 { Some(100) } else { None }
        );
    }
    f.accounts.reverse();
    assert_eq!(
        f.registry.capture_account_bundle(&mut f.accounts).unwrap(),
        image
    );
    assert_eq!(histories(&f), before);
    let metadata = format!("{report:?}");
    assert!(!metadata.contains("synthetic_user"));
    assert!(!metadata.contains("synthetic-password"));
    for (id, key) in &f.credentials {
        assert!(!metadata.contains(key));
        assert!(!image.windows(key.len()).any(|part| part == key.as_bytes()));
        assert_eq!(
            f.registry
                .authorize(id, key)
                .unwrap()
                .status()
                .unwrap()
                .rows,
            1
        );
    }
}

#[test]
fn empty_and_subset_rosters_are_explicit_and_do_not_discover_other_stores() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 2);
    let empty = f.registry.capture_account_bundle(&mut []).unwrap();
    let report = inspect_account_bundle_bytes(&empty).unwrap();
    assert_eq!(report.registry.projects.len(), 2);
    assert!(report.private_accounts.is_empty());
    let image = f
        .registry
        .capture_account_bundle(&mut f.accounts[..1])
        .unwrap();
    assert_eq!(
        inspect_account_bundle_bytes(&image)
            .unwrap()
            .private_accounts
            .len(),
        1
    );
    let mut empty_registry = ProjectStore::open(dir.path().join("empty")).unwrap();
    let image = empty_registry.capture_account_bundle(&mut []).unwrap();
    assert_eq!(image.len(), HEADER + registry_archive::HEADER);
    assert!(
        inspect_account_bundle_bytes(&image)
            .unwrap()
            .registry
            .projects
            .is_empty()
    );
}

#[test]
fn every_owner_is_retained_before_first_data_prefix_and_after_private_prefix() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    for boundary in [
        "registry_capture_owners_locked",
        "bundle_private_prefix_captured",
    ] {
        let data = f.data_paths.clone();
        let private = f.private_paths.clone();
        let credentials = f.credentials.clone();
        let pool = PasswordPool::new(1).unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let observed = called.clone();
        let _callback = durability::on_boundary(boundary, move || {
            for path in data {
                assert!(Database::open(path).is_err());
            }
            for (path, (id, _)) in private.iter().zip(&credentials) {
                assert!(AccountStore::open(path, id, pool.clone()).is_err());
            }
            observed.store(true, Ordering::SeqCst);
        });
        f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        assert!(called.load(Ordering::SeqCst));
        for path in &f.data_paths {
            assert!(Database::open(path).is_ok());
        }
    }
    let paths = f.private_paths.clone();
    drop(f.accounts);
    for (path, (id, _)) in paths.iter().zip(&f.credentials) {
        assert!(AccountStore::open(path, id, PasswordPool::new(1).unwrap()).is_ok());
    }
}

#[test]
fn invalid_rosters_and_competing_capabilities_fail_without_source_mutation() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 2);
    let before = histories(&f);
    let request = f
        .registry
        .authorize(&f.credentials[0].0, &f.credentials[0].1)
        .unwrap();
    assert!(matches!(
        f.registry.capture_account_bundle(&mut f.accounts),
        Err(Error::Busy)
    ));
    drop(request);
    let database = Database::open(&f.data_paths[1]).unwrap();
    assert!(f.registry.capture_account_bundle(&mut f.accounts).is_err());
    drop(database);
    // Any earlier data owner acquired before the error must also be released.
    assert!(Database::open(&f.data_paths[0]).is_ok());
    let duplicate = AccountStore::create(
        dir.path().join("duplicate"),
        &f.credentials[0].0,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap();
    f.accounts.push(duplicate);
    assert!(matches!(
        f.registry.capture_account_bundle(&mut f.accounts),
        Err(Error::BundleFormat(_))
    ));
    f.accounts.pop();
    let foreign = AccountStore::create(
        dir.path().join("foreign"),
        "00000000000000000000000000000000",
        PasswordPool::new(1).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        f.registry.capture_account_bundle(&mut [foreign]),
        Err(Error::BundleFormat(_))
    ));
    assert_eq!(histories(&f), before);
    assert!(f.registry.capture_account_bundle(&mut f.accounts).is_ok());
}

#[test]
fn source_metadata_is_rechecked_after_private_capture_and_substitute_is_preserved() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let path = dir
        .path()
        .join("registry")
        .join(&f.credentials[0].0)
        .join("project.json");
    let before = std::fs::read(&path).unwrap();
    let mut changed = crate::metadata::read(&path, &f.credentials[0].0).unwrap();
    changed.epoch += 1;
    let changed = crate::metadata::encoded(&changed).unwrap();
    let replacement = changed.clone();
    let selected = path.clone();
    let _callback = durability::on_boundary("bundle_private_prefix_captured", move || {
        std::fs::write(selected, replacement).unwrap();
    });
    assert!(matches!(
        f.registry.capture_account_bundle(&mut f.accounts),
        Err(Error::Metadata)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    for path in &f.data_paths {
        assert!(Database::open(path).is_ok());
    }
    std::fs::write(path, before).unwrap();
    assert!(f.registry.capture_account_bundle(&mut f.accounts).is_ok());
}

#[test]
fn framing_versions_checksums_reserved_fields_lengths_and_tail_are_strict() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    for size in [0, 7, 8, 127, 128, image.len() - 1] {
        assert!(inspect_account_bundle_bytes(&image[..size]).is_err());
    }
    for at in [
        0,
        8,
        10,
        12,
        16,
        24,
        32,
        64,
        123,
        124,
        HEADER,
        image.len() - 1,
    ] {
        let mut broken = image.clone();
        broken[at] ^= 0x80;
        assert!(
            inspect_account_bundle_bytes(&broken).is_err(),
            "unsealed offset {at}"
        );
    }
    for at in [0, 8, 10, 12, 16, 64, 123, HEADER] {
        let mut broken = image.clone();
        broken[at] ^= 0x80;
        seal(&mut broken);
        assert!(
            inspect_account_bundle_bytes(&broken).is_err(),
            "sealed offset {at}"
        );
    }
    let at = HEADER + u64_at(&image, 16) as usize;
    for length in [
        0,
        127,
        u64::MAX,
        (emilybase_backup::MAX_BACKUP_BYTES + 1) as u64,
    ] {
        let mut broken = image.clone();
        broken[at + 32..at + 40].copy_from_slice(&length.to_le_bytes());
        seal(&mut broken);
        assert!(inspect_account_bundle_bytes(&broken).is_err());
    }
    let mut extra = image.clone();
    extra.push(0);
    seal(&mut extra);
    assert!(matches!(
        inspect_account_bundle_bytes(&extra),
        Err(Error::BundleFormat("trailing payload"))
    ));
    let mut too_many = image.clone();
    too_many[12..16].copy_from_slice(&129_u32.to_le_bytes());
    seal(&mut too_many);
    assert!(inspect_account_bundle_bytes(&too_many).is_err());
    assert!(matches!(
        encode(&[], vec![(String::new(), vec![]); 129]),
        Err(Error::Limit)
    ));
    let mut unsupported = image.clone();
    unsupported[8..10].copy_from_slice(&2_u16.to_le_bytes());
    seal(&mut unsupported);
    assert!(matches!(
        inspect_account_bundle_bytes(&unsupported),
        Err(Error::BundleVersion(2))
    ));
}

#[test]
fn nested_scope_order_duplicate_and_complete_private_semantics_are_checked() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 2);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let (registry, entries) = parts(&image);
    let mut reversed = entries.clone();
    reversed.reverse();
    assert!(inspect_account_bundle_bytes(&raw(registry, &reversed)).is_err());
    let repeated = vec![entries[0].clone(), entries[0].clone()];
    assert!(inspect_account_bundle_bytes(&raw(registry, &repeated)).is_err());
    let mut foreign = entries.clone();
    foreign[0].0 = "00000000000000000000000000000000".into();
    assert!(inspect_account_bundle_bytes(&raw(registry, &foreign)).is_err());
    let mut wrong_scope = entries.clone();
    wrong_scope[0].1 = entries[1].1.clone();
    assert!(matches!(
        inspect_account_bundle_bytes(&raw(registry, &wrong_scope)),
        Err(Error::Accounts(
            emilybase_auth::accounts::Error::ScopeMismatch
        ))
    ));
    // Replace a valid private archive with a fully valid public data archive.
    let public = registry_archive::decode(registry).unwrap();
    let mut wrong_schema = entries.clone();
    wrong_schema[0].1 = public.entries[0].database.to_vec();
    assert!(matches!(
        inspect_account_bundle_bytes(&raw(registry, &wrong_schema)),
        Err(Error::Accounts(emilybase_auth::accounts::Error::Corrupt))
    ));
    // The checksum itself conveys neither authenticity nor common capture provenance.
    assert!(inspect_account_bundle_bytes(&raw(registry, &entries[..1])).is_ok());
}

#[test]
fn private_database_identity_cannot_alias_public_or_other_private_database() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 2);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let (registry, entries) = parts(&image);
    let public_id = crate::inspect_registry_backup_bytes(registry)
        .unwrap()
        .projects[0]
        .database_id;
    let private_id = inspect_private_account_backup_bytes(&entries[0].1, &entries[0].0)
        .unwrap()
        .database
        .database_id;
    for (index, identity) in [(0, public_id), (1, private_id)] {
        let mut alias = entries.clone();
        let verified = emilybase_backup::decode_verified(&alias[index].1).unwrap();
        let snapshot = &verified.image().snapshot;
        let pages = snapshot.pages().cloned().collect::<Vec<_>>();
        let wal = emilybase_wal::encode_snapshot(identity, 1, &pages).unwrap();
        alias[index].1 = emilybase_backup::encode(&wal).unwrap();
        assert!(inspect_private_account_backup_bytes(&alias[index].1, &alias[index].0).is_ok());
        assert!(matches!(
            inspect_account_bundle_bytes(&raw(registry, &alias)),
            Err(Error::BundleFormat("duplicate database identity"))
        ));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn roster_and_data_inventory_follow_an_independent_mutation_model(
        commands in prop::collection::vec((0_u8..4,0_u8..32),0..24), include in any::<bool>(), compact in any::<bool>()
    ) {
        let _serial = durability::PROCESS_TESTS.blocking_lock();
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(),1);
        let (id,key) = &f.credentials[0];
        let mut rows = std::collections::BTreeMap::from([(1_i64,0_i64)]);
        let mut disabled = false;
        let mut clock = 100_u64;
        f.accounts[0].enable_session_clock(clock).unwrap();
        for (kind,value) in commands {
            let value = i64::from(value);
            match kind {
                0 => {
                    let pk = value+2;
                    if rows.insert(pk,value).is_none() {
                        f.registry.authorize(id,key).unwrap().execute("INSERT INTO t VALUES($1,$2)",&[Value::Integer(pk),Value::Integer(value)]).unwrap();
                    }
                }
                1 => {
                    let pk = value+2;
                    f.registry.authorize(id,key).unwrap().execute("DELETE FROM t WHERE id=$1",&[Value::Integer(pk)]).unwrap();
                    rows.remove(&pk);
                }
                2 => {
                    disabled = !disabled;
                    f.accounts[0].set_disabled("synthetic_user",disabled).unwrap();
                }
                _ => {
                    clock += value as u64;
                    f.accounts[0].advance_session_clock(clock).unwrap();
                }
            }
        }
        if compact {
            f.accounts[0].compact().unwrap();
            Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
        }
        let before = histories(&f);
        let image = f.registry.capture_account_bundle(if include {&mut f.accounts} else {&mut []}).unwrap();
        let report = inspect_account_bundle_bytes(&image).unwrap();
        prop_assert_eq!(report.registry.projects[0].rows,rows.len());
        prop_assert_eq!(report.private_accounts.len(),usize::from(include));
        if include {
            prop_assert_eq!(report.private_accounts[0].inventory.clock_floor,Some(clock));
            prop_assert_eq!(report.private_accounts[0].inventory.accounts,1);
            prop_assert_eq!(report.private_accounts[0].inventory.database.wal_version,if compact {2} else {1});
        }
        prop_assert_eq!(histories(&f),before);
    }
}

#[test]
#[ignore = "synthetic corpus helper invoked explicitly by the fuzz campaign"]
fn bundle_corpus() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let output = PathBuf::from(std::env::var_os("EMILYBASE_BUNDLE_CORPUS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    for count in 0..=3 {
        let image = f
            .registry
            .capture_account_bundle(&mut f.accounts[..count])
            .unwrap();
        std::fs::write(output.join(format!("roster-{count}")), image).unwrap();
    }
    let mut empty = ProjectStore::open(dir.path().join("empty")).unwrap();
    std::fs::write(
        output.join("empty"),
        empty.capture_account_bundle(&mut []).unwrap(),
    )
    .unwrap();
}

#[test]
fn private_capture_failure_releases_data_owners_and_preserves_prior_prefixes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let before = histories(&f);
    let wal = f.private_paths[0].join("redo.wal");
    let mut broken = before[1].clone();
    let last = broken.len() - 1;
    broken[last] ^= 1;
    std::fs::write(&wal, &broken).unwrap();
    assert!(f.registry.capture_account_bundle(&mut f.accounts).is_err());
    assert_eq!(std::fs::read(&wal).unwrap(), broken);
    assert_eq!(
        std::fs::read(f.data_paths[0].join("redo.wal")).unwrap(),
        before[0]
    );
    assert!(Database::open(&f.data_paths[0]).is_ok());
    // A failed export does not transfer the caller's private writer owner.
    assert!(Database::open(&f.private_paths[0]).is_err());
    drop(f.accounts);
    std::fs::write(&wal, &before[1]).unwrap();
    assert!(
        AccountStore::open(
            &f.private_paths[0],
            &f.credentials[0].0,
            PasswordPool::new(1).unwrap()
        )
        .is_ok()
    );
}

#[test]
fn killed_common_capture_never_mutates_sources_and_releases_all_process_owners() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for boundary in [
            "registry_capture_owners_locked",
            "bundle_private_prefix_captured",
            "bundle_capture_ack",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut f = fixture(dir.path(), 1);
            if compact {
                f.accounts[0].compact().unwrap();
                Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
            }
            let before = histories(&f);
            let (id, key) = f.credentials[0].clone();
            let data_path = f.data_paths[0].clone();
            let private_path = f.private_paths[0].clone();
            drop(f);
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "account_bundle::tests::bundle_capture_worker",
                    "--nocapture",
                ])
                .env("EMILYBASE_BUNDLE_KILL_ROOT", dir.path())
                .env("EMILYBASE_BUNDLE_KILL_PROJECT", &id)
                .env("EMILYBASE_REGISTRY_KILL_POINT", boundary)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if send.send(line).is_err() {
                        break;
                    }
                }
            });
            let deadline = Instant::now() + Duration::from_secs(15);
            let reached = loop {
                match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(Ok(line)) if line == format!("REGISTRY_BOUNDARY {boundary}") => break true,
                    Ok(Ok(_)) => (),
                    _ => break false,
                }
            };
            if reached && boundary != "bundle_capture_ack" {
                assert!(Database::open(&data_path).is_err());
                assert!(
                    AccountStore::open(&private_path, &id, PasswordPool::new(1).unwrap()).is_err()
                );
            }
            child.kill().unwrap();
            assert!(!child.wait().unwrap().success());
            drop(receive);
            reader.join().unwrap();
            assert!(reached, "capture boundary was not reached: {boundary}");
            assert_eq!(
                std::fs::read(data_path.join("redo.wal")).unwrap(),
                before[0]
            );
            assert_eq!(
                std::fs::read(private_path.join("redo.wal")).unwrap(),
                before[1]
            );
            let mut registry = ProjectStore::open_existing(dir.path().join("registry")).unwrap();
            let mut accounts =
                [AccountStore::open(private_path, &id, PasswordPool::new(1).unwrap()).unwrap()];
            assert_eq!(
                registry
                    .authorize(&id, &key)
                    .unwrap()
                    .status()
                    .unwrap()
                    .rows,
                1
            );
            let report = inspect_account_bundle_bytes(
                &registry.capture_account_bundle(&mut accounts).unwrap(),
            )
            .unwrap();
            assert_eq!(report.private_accounts[0].inventory.accounts, 1);
        }
    }
}

#[test]
#[ignore = "child process paused at a capture boundary and forcibly killed"]
fn bundle_capture_worker() {
    let root = PathBuf::from(std::env::var_os("EMILYBASE_BUNDLE_KILL_ROOT").unwrap());
    let project = std::env::var("EMILYBASE_BUNDLE_KILL_PROJECT").unwrap();
    let mut registry = ProjectStore::open_existing(root.join("registry")).unwrap();
    let mut accounts = [AccountStore::open(
        root.join("private-0"),
        &project,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap()];
    let bytes = registry.capture_account_bundle(&mut accounts).unwrap();
    assert!(inspect_account_bundle_bytes(&bytes).is_ok());
    durability::checkpoint("bundle_capture_ack");
}

#[test]
fn total_size_admission_is_checked_before_retaining_another_private_image() {
    assert_eq!(
        extend_size(MAX_ACCOUNT_BUNDLE_BYTES - ENTRY - 128, 128).unwrap(),
        MAX_ACCOUNT_BUNDLE_BYTES
    );
    for (current, additional) in [
        (MAX_ACCOUNT_BUNDLE_BYTES - ENTRY - 127, 128),
        (usize::MAX, 0),
        (128, usize::MAX),
        (MAX_ACCOUNT_BUNDLE_BYTES, 0),
    ] {
        assert!(matches!(
            extend_size(current, additional),
            Err(Error::Limit)
        ));
    }
    let dir = tempfile::tempdir().unwrap();
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let mut f = fixture(dir.path(), 1);
    let image = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let (registry, entries) = parts(&image);
    assert_eq!(encode(registry, entries).unwrap(), image);
}
