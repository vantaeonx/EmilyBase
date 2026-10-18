use super::*;
use crate::{AccountFileRootReport, restore_account_file_archive_bytes_at};
use emilybase_auth::accounts::IssuedSession;
use std::io::Write;
mod backup;
mod input;
mod live;

struct Source {
    root: Restored,
    paths: Vec<PathBuf>,
    bytes: Vec<u8>,
    before: Vec<Vec<u8>>,
    tokens: Vec<IssuedSession>,
}
fn source(parent: &Path, count: usize, compact: bool) -> Source {
    let (f, _, paths) = prepare(parent, count, compact);
    let mut tokens = Vec::new();
    for (id, _) in &f.credentials {
        let mut account = AccountStore::open(f.root.join("private").join(id), id, pool()).unwrap();
        account.enable_row_policy_catalog().unwrap();
        let receipt = account.enable_public_admission_catalog().unwrap();
        account
            .set_public_admission(receipt.revision, true)
            .unwrap();
        tokens.push(account.sign_in(LOGIN, PASSWORD, 100).unwrap());
    }
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
    let bytes = encode(&snapshot);
    drop(files);
    drop(root);
    let before = history(&f, &paths);
    Source {
        root: f,
        paths,
        bytes,
        before,
        tokens,
    }
}
fn output(parent: &Path) -> PathBuf {
    let path = parent.join("output");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}
fn common_stage(parent: &Path) -> PathBuf {
    let entries = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".emilybase-directory-")
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    entries[0].clone()
}
fn verify_source(source: &Source) {
    assert_eq!(history(&source.root, &source.paths), source.before);
    for ((id, _), token) in source.root.credentials.iter().zip(&source.tokens) {
        let mut store =
            AccountStore::open(source.root.root.join("private").join(id), id, pool()).unwrap();
        assert!(store.verify_access(token.access.expose(), 100).is_ok());
        assert!(store.public_admission().unwrap().enabled);
    }
}
fn verify_copy(source: &Source, target: &Path, report: &AccountFileRootReport) {
    assert_eq!(report.manifest.reset_at(), 40);
    assert_eq!(report.private_accounts.len(), source.root.credentials.len());
    assert_eq!(report.files.len(), source.paths.len());
    assert!(AccountRoot::open(target, pool()).is_err());
    assert_eq!(
        crate::inspect_account_file_root_manifest_bytes(
            &fs::read(target.join("root.json")).unwrap()
        )
        .unwrap(),
        report.manifest
    );
    let registry = ProjectStore::open_existing(target.join("registry")).unwrap();
    for (index, ((id, key), old)) in source
        .root
        .credentials
        .iter()
        .zip(&source.tokens)
        .enumerate()
    {
        let rows = registry
            .authorize(id, key)
            .unwrap()
            .execute("SELECT id,v FROM t ORDER BY id", &[])
            .unwrap();
        assert_eq!(
            rows.results[0].rows,
            vec![vec![Value::Integer(1), Value::Integer(index as i64)]]
        );
        registry
            .authorize(id, key)
            .unwrap()
            .execute("INSERT INTO t VALUES(2,777)", &[])
            .unwrap();
        let mut account = AccountStore::open(target.join("private").join(id), id, pool()).unwrap();
        assert_eq!(account.session_clock_floor().unwrap(), Some(40));
        assert!(!account.public_admission().unwrap().enabled);
        assert!(account.verify_access(old.access.expose(), 100).is_err());
        assert!(account.refresh_session(old.refresh.expose(), 100).is_err());
        let new = account.sign_in(LOGIN, PASSWORD, 101).unwrap();
        assert!(account.verify_access(new.access.expose(), 101).is_ok());
        drop(account);
        let mut reopened = AccountStore::open(target.join("private").join(id), id, pool()).unwrap();
        assert!(reopened.verify_access(new.access.expose(), 101).is_ok());
    }
    for path in &source.paths {
        let id = path.file_name().unwrap().to_str().unwrap();
        let copied = target.join("files").join(id);
        assert_eq!(
            fs::read(object(&copied, false)).unwrap(),
            fs::read(object(path, false)).unwrap()
        );
        assert_eq!(
            fs::read(object(&copied, true)).unwrap(),
            fs::read(object(path, true)).unwrap()
        );
        let mut files = NativeFileRoot::open(&copied, id.parse().unwrap()).unwrap();
        let info = files.list().unwrap().remove(0);
        assert_eq!(info.name(), "synthetic-name");
        files.rename(FILE, info.revision(), "copied-name").unwrap();
        assert_eq!(files.list().unwrap().remove(0).name(), "copied-name");
    }
    assert_eq!(fs::read_dir(target).unwrap().count(), 4);
}
#[test]
fn common_v2_restore_preserves_all_components_resets_only_copy_and_accepts_independent_writes() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let temp = tempfile::tempdir().unwrap();
        let source = source(temp.path(), count, compact);
        let out = output(temp.path());
        let parent = File::open(&out).unwrap();
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let report =
            restore_account_file_archive_bytes_at(&source.bytes, &parent, "copy", pool(), 40)
                .unwrap();
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        verify_copy(&source, &out.join("copy"), &report);
        verify_source(&source);
    }
}
#[test]
fn all_original_writers_span_selection_and_readers_with_common_lock_span_final_unwind() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for phase in [
        "joined_restore_prepared",
        "joined_restore_selected",
        "joined_restore_finalizing",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = source(temp.path(), 2, true);
        let out = output(temp.path());
        let root = out.clone();
        let ids = source
            .root
            .credentials
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let _hook = durability::on_boundary(phase, move || {
            let path = if phase == "joined_restore_prepared" {
                common_stage(&root)
            } else {
                root.join("copy")
            };
            assert!(File::open(&path).unwrap().try_lock().is_err());
            for id in ids {
                for wal_parent in [
                    path.join("registry").join(&id).join("data"),
                    path.join("private").join(&id),
                    path.join("files").join(&id).join("metadata"),
                ] {
                    if phase == "joined_restore_finalizing" {
                        drop(Database::open(&wal_parent).unwrap());
                    } else {
                        assert!(Database::open(&wal_parent).is_err());
                    }
                    assert!(
                        fds(std::process::id(), &wal_parent.join("redo.wal"))
                            >= if phase == "joined_restore_finalizing" {
                                1
                            } else {
                                2
                            }
                    );
                }
                assert!(
                    fds(
                        std::process::id(),
                        &object(&path.join("files").join(&id), false)
                    ) >= if phase == "joined_restore_finalizing" {
                        1
                    } else {
                        2
                    }
                );
            }
        });
        restore_account_file_archive_bytes_at(
            &source.bytes,
            &File::open(&out).unwrap(),
            "copy",
            pool(),
            40,
        )
        .unwrap();
        File::open(out.join("copy")).unwrap().try_lock().unwrap();
        verify_source(&source);
    }
}
fn change(root: &Path, out: &Path, id: &str, index: usize) {
    let private = root.join("private").join(id).join("redo.wal");
    let public = root.join("registry").join(id).join("data/redo.wal");
    let files = root.join("files").join(id);
    match index {
        0 => {
            let saved = out.join("original-wal");
            fs::rename(&private, &saved).unwrap();
            fs::copy(saved, &private).unwrap();
        }
        1 => File::options()
            .append(true)
            .open(public)
            .unwrap()
            .write_all(b"unacknowledged external tail")
            .unwrap(),
        2 => fs::write(files.join("metadata/redo.wal"), b"changed metadata").unwrap(),
        3 => fs::write(object(&files, false), b"changed physical payload").unwrap(),
        4 => File::options()
            .append(true)
            .open(object(&files, true))
            .unwrap()
            .write_all(b"orphan tail")
            .unwrap(),
        5 => {
            let manifest = root.join("root.json");
            let saved = out.join("original-manifest");
            fs::rename(&manifest, &saved).unwrap();
            fs::copy(saved, &manifest).unwrap();
        }
        6 => {
            fs::rename(root.join("private"), out.join("original-private")).unwrap();
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join("private"))
                .unwrap();
        }
        7 => fs::set_permissions(
            root.join("registry").join(id).join("project.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap(),
        8 => fs::set_permissions(root, fs::Permissions::from_mode(0o755)).unwrap(),
        9 => fs::set_permissions(out, fs::Permissions::from_mode(0o755)).unwrap(),
        10 => fs::write(root.join("foreign"), b"keep").unwrap(),
        _ => fs::hard_link(private, out.join("original-link")).unwrap(),
    }
}
#[test]
fn pre_selection_and_late_physical_changes_withhold_common_receipt_and_preserve_original_sources() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = source(temp.path(), 1, compact);
        for phase in [
            "joined_restore_prepared",
            "joined_restore_selected",
            "joined_restore_finalizing",
        ] {
            for index in 0..12 {
                let out = temp.path().join(format!("{phase}-{index}"));
                fs::DirBuilder::new().mode(0o700).create(&out).unwrap();
                let root = out.clone();
                let id = source.root.credentials[0].0.clone();
                let _hook = durability::on_boundary(phase, move || {
                    let selected = if phase == "joined_restore_prepared" {
                        common_stage(&root)
                    } else {
                        root.join("copy")
                    };
                    change(&selected, &root, &id, index);
                });
                let error = restore_account_file_archive_bytes_at(
                    &source.bytes,
                    &File::open(&out).unwrap(),
                    "copy",
                    pool(),
                    40,
                )
                .unwrap_err();
                assert_eq!(
                    matches!(error, Error::ComponentRestoreUnknown(_)),
                    phase != "joined_restore_prepared",
                    "phase={phase},change={index},error={error:?}"
                );
                assert_eq!(
                    out.join("copy").exists(),
                    phase != "joined_restore_prepared"
                );
                verify_source(&source);
            }
        }
    }
}
#[test]
fn sync_faults_distinguish_private_stage_from_selected_uncertainty() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path(), 1, false);
    for (phase, late, selected) in [
        (
            "joined_restore_manifest_sync",
            "joined_restore_manifest_sync_after",
            false,
        ),
        (
            "joined_restore_private_sync",
            "joined_restore_private_sync_after",
            false,
        ),
        (
            "joined_restore_files_sync",
            "joined_restore_files_sync_after",
            false,
        ),
        (
            "joined_restore_manifest_confirm_sync",
            "joined_restore_manifest_confirm_sync_after",
            false,
        ),
        (
            "joined_restore_confirm_parent_sync",
            "joined_restore_confirm_parent_sync_after",
            true,
        ),
    ] {
        for fault in [phase, late] {
            let out = temp.path().join(fault);
            fs::DirBuilder::new().mode(0o700).create(&out).unwrap();
            durability::inject(fault);
            let result = restore_account_file_archive_bytes_at(
                &source.bytes,
                &File::open(&out).unwrap(),
                "copy",
                pool(),
                40,
            );
            assert_eq!(
                matches!(result, Err(Error::ComponentRestoreUnknown(_))),
                selected
            );
            assert_eq!(out.join("copy").exists(), selected);
            assert!(result.is_err());
            verify_source(&source);
        }
    }
}
#[test]
fn invalid_inputs_private_parent_and_occupied_targets_refuse_without_overwriting() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path(), 0, false);
    let out = output(temp.path());
    let parent = File::open(&out).unwrap();
    for bad in [b"".as_slice(), b"invalid joined image"] {
        assert!(restore_account_file_archive_bytes_at(bad, &parent, "copy", pool(), 40).is_err());
    }
    assert!(
        restore_account_file_archive_bytes_at(&source.bytes, &parent, "copy", pool(), u64::MAX)
            .is_err()
    );
    let long = "x".repeat(256);
    for name in ["", ".", "..", "../escape", "a/b", "nul\0leaf", &long] {
        assert!(
            restore_account_file_archive_bytes_at(&source.bytes, &parent, name, pool(), 40)
                .is_err()
        );
    }
    for mode in [0o755, 0o770, 0o1700] {
        fs::set_permissions(&out, fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            restore_account_file_archive_bytes_at(&source.bytes, &parent, "copy", pool(), 40)
                .is_err()
        );
    }
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    fs::write(out.join("file"), b"keep").unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(out.join("directory"))
        .unwrap();
    symlink(temp.path(), out.join("alias")).unwrap();
    for name in ["file", "directory", "alias"] {
        assert!(
            restore_account_file_archive_bytes_at(&source.bytes, &parent, name, pool(), 40)
                .is_err()
        );
    }
    assert_eq!(fs::read(out.join("file")).unwrap(), b"keep");
    assert_eq!(fs::read_link(out.join("alias")).unwrap(), temp.path());
    assert_eq!(fs::read_dir(out.join("directory")).unwrap().count(), 0);
    verify_source(&source);
}
#[test]
fn moved_original_parent_keeps_actual_selected_root_and_ignores_foreign_old_path() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path(), 1, true);
    let out = output(temp.path());
    let moved = temp.path().join("moved");
    let parent = File::open(&out).unwrap();
    let old = out.clone();
    let new = moved.clone();
    let _hook = durability::on_boundary("joined_restore_selected", move || {
        fs::rename(&old, &new).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&old).unwrap();
        fs::write(old.join("keep"), b"foreign").unwrap();
    });
    let report =
        restore_account_file_archive_bytes_at(&source.bytes, &parent, "copy", pool(), 40).unwrap();
    assert_eq!(fs::read(out.join("keep")).unwrap(), b"foreign");
    assert!(!out.join("copy").exists());
    verify_copy(&source, &moved.join("copy"), &report);
    verify_source(&source);
}
fn generated(count: usize, compact: bool, omit_files: bool) {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, _, paths) = prepare(temp.path(), count, compact);
    let before = history(&f, &paths);
    let mut root = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    if omit_files {
        files.clear();
    }
    let snapshot = capture_account_file_components(&mut root, &mut files).unwrap();
    let bytes = encode(&snapshot);
    drop(files);
    drop(root);
    let out = output(temp.path());
    let report = restore_account_file_archive_bytes_at(
        &bytes,
        &File::open(&out).unwrap(),
        "copy",
        pool(),
        40,
    )
    .unwrap();
    assert_eq!(report.registry.projects.len(), count);
    assert_eq!(report.private_accounts.len(), count);
    assert_eq!(report.files.len(), if omit_files { 0 } else { count });
    assert_eq!(history(&f, &paths), before);
    for account in &report.private_accounts {
        assert_eq!(account.inventory.clock_floor, Some(40));
        assert_eq!(account.inventory.private_version, 3);
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_mixed_rosters_preserve_exact_counts_and_sources(count in 0usize..3, compact in any::<bool>(), omit_files in any::<bool>()) {
        generated(count,compact,omit_files);
    }
}

#[test]
fn actual_kills_preserve_private_stages_or_selected_common_roots_and_release_original_owners() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (2, true)] {
        let temp = tempfile::tempdir().unwrap();
        let source = source(temp.path(), count, compact);
        let archive = temp.path().join("sensitive.backup");
        fs::write(&archive, &source.bytes).unwrap();
        let mut phases = vec![
            "joined_restore_registry_ready",
            "joined_restore_prepared",
            "joined_restore_selected",
            "joined_restore_finalizing",
            "joined_restore_ack",
        ];
        if count != 0 {
            phases.extend(["joined_restore_private_ready", "joined_restore_file_ready"]);
        }
        for phase in phases {
            let out = temp.path().join(phase);
            fs::DirBuilder::new().mode(0o700).create(&out).unwrap();
            let target = out.join("copy");
            let worker = Worker::start(&archive, &target, "", "joined-restore", phase);
            worker.reach(phase);
            let selected = matches!(
                phase,
                "joined_restore_selected" | "joined_restore_finalizing" | "joined_restore_ack"
            );
            assert_eq!(target.exists(), selected);
            let path = if selected {
                target.clone()
            } else {
                common_stage(&out)
            };
            if phase == "joined_restore_ack" {
                File::open(&path).unwrap().try_lock().unwrap();
            } else {
                assert!(File::open(&path).unwrap().try_lock().is_err());
            }
            let complete = matches!(
                phase,
                "joined_restore_prepared"
                    | "joined_restore_selected"
                    | "joined_restore_finalizing"
                    | "joined_restore_ack"
            );
            if complete {
                for (id, _) in &source.root.credentials {
                    let wal = path.join("private").join(id).join("redo.wal");
                    assert_eq!(fds(worker.pid(), &wal) == 0, phase == "joined_restore_ack");
                    let blob = object(&path.join("files").join(id), false);
                    assert_eq!(fds(worker.pid(), &blob) == 0, phase == "joined_restore_ack");
                }
            }
            worker.kill();
            File::open(&path).unwrap().try_lock().unwrap();
            if complete {
                let manifest = crate::inspect_account_file_root_manifest_bytes(
                    &fs::read(path.join("root.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(manifest.reset_at(), 40);
                for ((id, _), token) in source.root.credentials.iter().zip(&source.tokens) {
                    let mut account =
                        AccountStore::open(path.join("private").join(id), id, pool()).unwrap();
                    assert_eq!(account.session_clock_floor().unwrap(), Some(40));
                    assert!(!account.public_admission().unwrap().enabled);
                    assert!(account.verify_access(token.access.expose(), 100).is_err());
                    let fresh = account.sign_in(LOGIN, PASSWORD, 101).unwrap();
                    assert!(account.verify_access(fresh.access.expose(), 101).is_ok());
                    let mut files =
                        NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap())
                            .unwrap();
                    assert_eq!(files.list().unwrap().len(), 1);
                }
            }
            assert_eq!(fs::read(&archive).unwrap(), source.bytes);
            verify_source(&source);
            let report = restore_account_file_archive_bytes_at(
                &source.bytes,
                &File::open(&out).unwrap(),
                "fresh-retry",
                pool(),
                40,
            )
            .unwrap();
            verify_copy(&source, &out.join("fresh-retry"), &report);
            verify_source(&source);
        }
    }
}
