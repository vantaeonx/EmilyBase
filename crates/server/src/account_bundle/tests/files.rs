use super::{fixture, histories};
use crate::account_bundle::{MAX_ACCOUNT_BUNDLE_BYTES, files::publish};
use crate::{
    Error, ProjectStore, durability, inspect_account_bundle, inspect_account_bundle_bytes,
};
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_catalog::Value;
use emilybase_transactions::Database;
use proptest::prelude::*;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

const PREFIX: &str = ".emilybase-account-bundle-";
fn staging(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(PREFIX))
        .collect()
}
fn private_write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn public_file_export_reads_back_exact_sensitive_image_without_changing_sources() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    let expected = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let prior = histories(&f);
    let target = dir.path().join("synthetic.account-bundle");
    let report = f
        .registry
        .backup_account_bundle(&mut f.accounts, &target)
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), expected);
    assert_eq!(inspect_account_bundle(&target).unwrap(), report);
    assert_eq!(inspect_account_bundle_bytes(&expected).unwrap(), report);
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(histories(&f), prior);
    assert!(staging(dir.path()).is_empty());
    for path in &f.data_paths {
        assert!(Database::open(path).is_ok());
    }
    for path in &f.private_paths {
        assert!(Database::open(path).is_err());
    }
}

#[test]
fn export_never_replaces_existing_files_directories_or_links_and_refuses_registry_targets() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let prior = histories(&f);
    let protected = dir.path().join("protected");
    private_write(&protected, b"synthetic protected bytes");
    let link = dir.path().join("link");
    symlink(&protected, &link).unwrap();
    let directory = dir.path().join("directory");
    fs::create_dir(&directory).unwrap();
    for target in [
        &protected,
        &link,
        &directory,
        &f.private_paths[0].join("redo.wal"),
    ] {
        assert!(
            f.registry
                .backup_account_bundle(&mut f.accounts, target)
                .is_err()
        );
    }
    let inside = f.data_paths[0].join("forbidden.account-bundle");
    assert!(matches!(
        f.registry.backup_account_bundle(&mut f.accounts, &inside),
        Err(Error::Path)
    ));
    assert!(!inside.exists());
    assert_eq!(fs::read(protected).unwrap(), b"synthetic protected bytes");
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    assert_eq!(histories(&f), prior);
    assert!(staging(dir.path()).is_empty());
}

#[test]
fn readers_refuse_nonregular_alias_broad_permission_oversized_and_invalid_files() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let target = dir.path().join("private.account-bundle");
    f.registry
        .backup_account_bundle(&mut f.accounts, &target)
        .unwrap();
    let prior = fs::read(&target).unwrap();
    let alias = dir.path().join("alias");
    fs::hard_link(&target, &alias).unwrap();
    assert!(matches!(inspect_account_bundle(&target), Err(Error::Path)));
    fs::remove_file(alias).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(inspect_account_bundle(&target), Err(Error::Path)));
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("symlink");
    symlink(&target, &link).unwrap();
    assert!(inspect_account_bundle(&link).is_err());
    assert!(matches!(
        inspect_account_bundle(dir.path()),
        Err(Error::Path)
    ));
    let fifo = dir.path().join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    assert!(matches!(inspect_account_bundle(&fifo), Err(Error::Path)));
    let sparse = dir.path().join("oversized");
    private_write(&sparse, &[]);
    File::options()
        .write(true)
        .open(&sparse)
        .unwrap()
        .set_len(MAX_ACCOUNT_BUNDLE_BYTES as u64 + 1)
        .unwrap();
    assert!(matches!(inspect_account_bundle(&sparse), Err(Error::Limit)));
    let invalid = dir.path().join("invalid");
    private_write(&invalid, b"not an archive");
    assert!(inspect_account_bundle(invalid).is_err());
    assert_eq!(fs::read(target).unwrap(), prior);
}

#[test]
fn malformed_bytes_fail_before_creating_staging_or_touching_destination() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("selected.account-bundle");
    for bytes in [vec![], vec![0; 128], vec![0; 1024 * 1024]] {
        assert!(publish(&bytes, &target).is_err());
        assert!(!target.exists());
        assert!(staging(dir.path()).is_empty());
    }
    private_write(&target, b"preserve");
    assert!(publish(&[0; 128], &target).is_err());
    assert_eq!(fs::read(target).unwrap(), b"preserve");
}

#[test]
fn bundle_sync_faults_distinguish_no_publication_from_selected_uncertainty() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    for compact in [false, true] {
        if compact {
            f.accounts[0].compact().unwrap();
            Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
        }
        let before = histories(&f);
        let expected = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        for (index, point) in [
            "bundle_backup_file_sync",
            "bundle_backup_file_sync_after",
            "bundle_backup_parent_sync",
            "bundle_backup_parent_sync_after",
        ]
        .into_iter()
        .enumerate()
        {
            let target = dir
                .path()
                .join(format!("fault-{compact}-{index}.account-bundle"));
            durability::inject(point);
            let result = f.registry.backup_account_bundle(&mut f.accounts, &target);
            let selected = point.contains("parent");
            if selected {
                assert!(matches!(result, Err(Error::PublicationUnknown(_))));
                assert_eq!(fs::read(&target).unwrap(), expected);
                assert!(inspect_account_bundle(&target).is_ok());
            } else {
                assert!(matches!(result, Err(Error::Io(_))));
                assert!(!target.exists());
                f.registry
                    .backup_account_bundle(&mut f.accounts, &target)
                    .unwrap();
            }
            assert_eq!(histories(&f), before);
            assert!(staging(dir.path()).is_empty());
        }
    }
}

#[test]
fn parent_substitution_preserves_foreign_entries_before_and_after_publication() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in ["bundle_backup_file_synced", "bundle_backup_renamed"] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        let prior = histories(&f);
        let expected = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let parent = dir.path().join("outputs");
        let moved = dir.path().join("moved");
        fs::create_dir(&parent).unwrap();
        let selected = parent.join("selected.account-bundle");
        let callback_parent = parent.clone();
        let callback_moved = moved.clone();
        let _guard = durability::on_boundary(point, move || {
            let stage = staging(&callback_parent)
                .into_iter()
                .next()
                .and_then(|p| p.file_name().map(|n| n.to_owned()));
            fs::rename(&callback_parent, &callback_moved).unwrap();
            fs::create_dir(&callback_parent).unwrap();
            private_write(
                &callback_parent.join("selected.account-bundle"),
                b"foreign selected",
            );
            if let Some(name) = stage {
                private_write(&callback_parent.join(name), b"foreign staged");
            }
        });
        let result = f.registry.backup_account_bundle(&mut f.accounts, &selected);
        if point == "bundle_backup_renamed" {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            assert_eq!(
                fs::read(moved.join("selected.account-bundle")).unwrap(),
                expected
            );
        } else {
            assert!(matches!(result, Err(Error::Path)));
            assert!(!moved.join("selected.account-bundle").exists());
            let foreign = staging(&parent);
            assert_eq!(foreign.len(), 1);
            assert_eq!(fs::read(&foreign[0]).unwrap(), b"foreign staged");
        }
        assert_eq!(fs::read(selected).unwrap(), b"foreign selected");
        assert_eq!(histories(&f), prior);
    }
}

#[test]
fn substituted_staging_and_selected_inode_are_never_deleted_or_adopted() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for selected in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        let expected = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let target = dir.path().join("selected.account-bundle");
        let detached = dir.path().join("detached");
        let callback_parent = dir.path().to_path_buf();
        let callback_target = target.clone();
        let callback_detached = detached.clone();
        let point = if selected {
            "bundle_backup_renamed"
        } else {
            "bundle_backup_file_synced"
        };
        let _guard = durability::on_boundary(point, move || {
            let path = if selected {
                callback_target
            } else {
                staging(&callback_parent).pop().unwrap()
            };
            fs::rename(&path, &callback_detached).unwrap();
            private_write(&path, b"foreign replacement");
        });
        let result = f.registry.backup_account_bundle(&mut f.accounts, &target);
        if selected {
            assert!(matches!(result, Err(Error::PublicationUnknown(_))));
            assert_eq!(fs::read(target).unwrap(), b"foreign replacement");
        } else {
            assert!(matches!(result, Err(Error::Path)));
            assert!(!target.exists());
            assert_eq!(
                fs::read(staging(dir.path()).pop().unwrap()).unwrap(),
                b"foreign replacement"
            );
        }
        assert_eq!(fs::read(detached).unwrap(), expected);
    }
}

#[test]
fn changed_staged_contents_or_permissions_are_rejected_before_rename() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for permissions in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        let before = histories(&f);
        let parent = dir.path().to_owned();
        let _guard = durability::on_boundary("bundle_backup_file_synced", move || {
            let stage = staging(&parent).pop().unwrap();
            if permissions {
                fs::set_permissions(stage, fs::Permissions::from_mode(0o644)).unwrap();
            } else {
                private_write(&stage, b"changed staged bytes");
            }
        });
        let target = dir.path().join("selected.account-bundle");
        assert!(
            f.registry
                .backup_account_bundle(&mut f.accounts, &target)
                .is_err()
        );
        assert!(!target.exists());
        assert!(staging(dir.path()).is_empty());
        assert_eq!(histories(&f), before);
    }
}

#[test]
fn final_parent_symlink_is_refused_without_writing_through_it() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 1);
    let parent = dir.path().join("parent");
    let alias = dir.path().join("alias");
    fs::create_dir(&parent).unwrap();
    symlink(&parent, &alias).unwrap();
    assert!(
        f.registry
            .backup_account_bundle(&mut f.accounts, alias.join("selected.account-bundle"))
            .is_err()
    );
    assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
}

pub(super) struct Worker {
    child: Child,
    lines: Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub(super) fn start(
        source: &Path,
        target: &Path,
        project: &str,
        action: &str,
        point: &str,
    ) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "account_bundle::tests::files::publication_worker",
                "--nocapture",
            ])
            .env("EMILYBASE_BUNDLE_FILE_SOURCE", source)
            .env("EMILYBASE_BUNDLE_FILE_TARGET", target)
            .env("EMILYBASE_BUNDLE_FILE_PROJECT", project)
            .env("EMILYBASE_BUNDLE_FILE_ACTION", action)
            .env("EMILYBASE_REGISTRY_KILL_POINT", point)
            .env(
                "EMILYBASE_REGISTRY_RESUME_POINT",
                if action.ends_with("race") { point } else { "" },
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            lines,
            reader: Some(reader),
        }
    }
    pub(super) fn reach(&self, point: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let line = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if line == format!("REGISTRY_BOUNDARY {point}") {
                break;
            }
        }
    }
    pub(super) fn kill(mut self) {
        self.child.kill().unwrap();
        assert!(!self.child.wait().unwrap().success());
        self.reader.take().unwrap().join().unwrap();
    }
    pub(super) fn release(&mut self) {
        self.child.stdin.take().unwrap().write_all(b"1").unwrap();
    }
    pub(super) fn finish(mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        self.reader.take().unwrap().join().unwrap();
        self.lines.try_iter().collect::<Vec<_>>().join("\n")
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
#[ignore = "publication helper invoked by native child tests"]
fn publication_worker() {
    let source = PathBuf::from(std::env::var_os("EMILYBASE_BUNDLE_FILE_SOURCE").unwrap());
    let target = PathBuf::from(std::env::var_os("EMILYBASE_BUNDLE_FILE_TARGET").unwrap());
    let project = std::env::var("EMILYBASE_BUNDLE_FILE_PROJECT").unwrap();
    let action = std::env::var("EMILYBASE_BUNDLE_FILE_ACTION").unwrap();
    if action.starts_with("root-init") {
        match crate::initialize_account_root(
            &target,
            "synthetic",
            PasswordPool::new(1).unwrap(),
            50,
        ) {
            Ok(_) => println!("BUNDLE_OK"),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                println!("BUNDLE_CONFLICT")
            }
            Err(_) => panic!("unexpected root initialization result"),
        }
        durability::checkpoint("account_init_ack");
    } else if action == "root-live" {
        let key: String = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
        let mut root = crate::AccountRoot::open(&source, PasswordPool::new(1).unwrap()).unwrap();
        let pair = root
            .sign_in(&project, &key, "synthetic_user", b"synthetic-password", 50)
            .unwrap();
        private_write(
            &target.with_extension("session"),
            &serde_json::to_vec(&(pair.access.expose(), pair.refresh.expose())).unwrap(),
        );
        File::open(target.with_extension("session"))
            .unwrap()
            .sync_all()
            .unwrap();
        durability::checkpoint("account_root_service_ack");
    } else if action.starts_with("root-backup") {
        match crate::backup_account_bundle_root(&source, &target, PasswordPool::new(1).unwrap()) {
            Ok(_) => println!("BUNDLE_OK"),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                println!("BUNDLE_CONFLICT")
            }
            Err(_) => panic!("unexpected root backup result"),
        }
        durability::checkpoint("bundle_backup_ack");
    } else if action.starts_with("root") {
        match crate::restore_account_bundle(&source, &target, PasswordPool::new(1).unwrap(), 50) {
            Ok(_) => println!("BUNDLE_OK"),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                println!("BUNDLE_CONFLICT")
            }
            Err(_) => panic!("unexpected root restore result"),
        }
        durability::checkpoint("bundle_restore_ack");
    } else if action == "race" {
        let bytes = crate::registry_files::read_bounded(&source, MAX_ACCOUNT_BUNDLE_BYTES).unwrap();
        let result = publish(&bytes, &target);
        match result {
            Ok(_) => println!("BUNDLE_OK"),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                println!("BUNDLE_CONFLICT")
            }
            Err(_) => panic!("unexpected bundle publication result"),
        }
    } else {
        let mut registry = ProjectStore::open_existing(source.join("registry")).unwrap();
        let mut accounts = [AccountStore::open(
            source.join("private-0"),
            &project,
            PasswordPool::new(1).unwrap(),
        )
        .unwrap()];
        registry
            .backup_account_bundle(&mut accounts, target)
            .unwrap();
        durability::checkpoint("bundle_backup_ack");
    }
}

#[test]
fn process_kills_never_select_partial_bundle_files_and_preserve_acknowledged_sources() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = fixture(dir.path(), 1);
        f.accounts[0].enable_session_clock(100).unwrap();
        if compact {
            f.accounts[0].compact().unwrap();
            Database::open(&f.data_paths[0]).unwrap().compact().unwrap();
        }
        let expected = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
        let before = histories(&f);
        let data = f.data_paths[0].clone();
        let private = f.private_paths[0].clone();
        let (id, key) = f.credentials[0].clone();
        drop(f);
        for point in [
            "registry_capture_owners_locked",
            "bundle_private_prefix_captured",
            "bundle_backup_file_synced",
            "bundle_backup_renamed",
            "bundle_backup_parent_synced",
            "bundle_backup_ack",
        ] {
            let target = dir.path().join(format!("{point}.account-bundle"));
            let worker = Worker::start(dir.path(), &target, &id, "capture", point);
            worker.reach(point);
            worker.kill();
            let selected = matches!(
                point,
                "bundle_backup_renamed" | "bundle_backup_parent_synced" | "bundle_backup_ack"
            );
            assert_eq!(target.exists(), selected);
            if selected {
                assert_eq!(fs::read(&target).unwrap(), expected);
                assert!(inspect_account_bundle(&target).is_ok());
            }
            assert_eq!(fs::read(data.join("redo.wal")).unwrap(), before[0]);
            assert_eq!(fs::read(private.join("redo.wal")).unwrap(), before[1]);
            let registry = ProjectStore::open_existing(dir.path().join("registry")).unwrap();
            assert_eq!(
                registry
                    .authorize(&id, &key)
                    .unwrap()
                    .status()
                    .unwrap()
                    .rows,
                1
            );
            let account = AccountStore::open(&private, &id, PasswordPool::new(1).unwrap()).unwrap();
            assert_eq!(account.count().unwrap(), 1);
        }
    }
}

#[test]
fn two_synchronized_native_publishers_select_exactly_one_complete_archive() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path(), 3);
    let expected = f.registry.capture_account_bundle(&mut f.accounts).unwrap();
    let input = dir.path().join("input.account-bundle");
    private_write(&input, &expected);
    let target = dir.path().join("selected.account-bundle");
    let point = "bundle_backup_file_synced";
    let mut first = Worker::start(&input, &target, "", "race", point);
    let mut second = Worker::start(&input, &target, "", "race", point);
    first.reach(point);
    second.reach(point);
    first.release();
    second.release();
    let results = [first.finish(), second.finish()];
    assert_eq!(
        results.iter().filter(|s| s.contains("BUNDLE_OK")).count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|s| s.contains("BUNDLE_CONFLICT"))
            .count(),
        1
    );
    assert_eq!(fs::read(&target).unwrap(), expected);
    assert_eq!(fs::read(input).unwrap(), expected);
    assert!(inspect_account_bundle(target).is_ok());
    assert!(staging(dir.path()).is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn independently_modeled_rows_survive_failed_publication_and_verified_retry(
        values in prop::collection::btree_set(2_i64..64,0..16), before_sync in any::<bool>(),compact in any::<bool>()
    ) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();
        let dir=tempfile::tempdir().unwrap();
        let mut f=fixture(dir.path(),1);
        for value in &values {
            f.registry.authorize(&f.credentials[0].0,&f.credentials[0].1).unwrap()
                .execute("INSERT INTO t VALUES($1,$1)",&[Value::Integer(*value)]).unwrap();
        }
        if compact {f.accounts[0].compact().unwrap();Database::open(&f.data_paths[0]).unwrap().compact().unwrap();}
        let before=histories(&f);
        let target=dir.path().join("selected.account-bundle");
        durability::inject(if before_sync {"bundle_backup_file_sync"} else {"bundle_backup_parent_sync_after"});
        let failed=f.registry.backup_account_bundle(&mut f.accounts,&target);
        prop_assert!(failed.is_err());
        if before_sync {f.registry.backup_account_bundle(&mut f.accounts,&target).unwrap();}
        let report=inspect_account_bundle(&target).unwrap();
        prop_assert_eq!(report.registry.projects[0].rows,values.len()+1);
        prop_assert_eq!(report.private_accounts[0].inventory.accounts,1);
        prop_assert_eq!(histories(&f),before);
        let bytes=fs::read(&target).unwrap();
        prop_assert!(f.registry.backup_account_bundle(&mut f.accounts,&target).is_err());
        prop_assert_eq!(fs::read(target).unwrap(),bytes);
        prop_assert!(staging(dir.path()).is_empty());
    }
}
