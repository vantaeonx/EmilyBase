use super::super::super::super::at::copy_directory;
use super::*;
mod http;
use crate::{AccountFileRootManifest, FileRootBinding, backup_account_file_components};

fn copy(parent: &Path, count: usize, compact: bool) -> (Source, PathBuf) {
    let source = source(parent, count, compact);
    let out = output(parent);
    let path = out.join("v2");
    restore_account_file_archive_bytes_at(
        &source.bytes,
        &File::open(&out).unwrap(),
        "v2",
        pool(),
        40,
    )
    .unwrap();
    (source, path)
}
fn copied_history(source: &Source, path: &Path) -> Vec<Vec<u8>> {
    let restored = Restored {
        root: path.into(),
        credentials: source.root.credentials.clone(),
    };
    let files = source
        .paths
        .iter()
        .map(|p| path.join("files").join(p.file_name().unwrap()))
        .collect::<Vec<_>>();
    history(&restored, &files)
}
fn manifest(path: &Path) -> AccountFileRootManifest {
    crate::inspect_account_file_root_manifest_bytes(&fs::read(path.join("root.json")).unwrap())
        .unwrap()
}
#[test]
fn explicit_v2_open_retains_real_file_owners_preserves_restarts_and_executes_account_and_public_writes()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), count, compact);
        let before = copied_history(&s, &path);
        let expected = manifest(&path);
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        assert!(AccountRoot::open(&path, pool()).is_err());
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert_eq!(root.file_manifest().unwrap(), Some(expected.clone()));
        assert_eq!(
            root.private_projects().unwrap(),
            expected.private_projects()
        );
        assert_eq!(root.projects().unwrap().len(), count);
        assert!(matches!(
            AccountRoot::open_v2(&path, pool()),
            Err(Error::Busy)
        ));
        assert!(ProjectStore::open_existing(path.join("registry")).is_err());
        for (id, _) in &s.root.credentials {
            assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
            assert!(
                NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap()).is_err()
            );
            // Public writers are scoped to validation/operations, as in v1.
            assert!(Database::open(path.join("registry").join(id).join("data")).is_ok());
        }
        assert_eq!(copied_history(&s, &path), before);
        let mut tokens = Vec::new();
        for ((id, key), old) in s.root.credentials.iter().zip(&s.tokens) {
            assert!(
                root.with_access(id, key, old.access.expose(), 100, |_| ())
                    .is_err()
            );
            let session = root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap();
            root.with_access(id, key, session.access.expose(), 101, |_| ())
                .unwrap();
            tokens.push(session);
            root.execute(id, key, "UPDATE t SET v=123 WHERE id=1", &[])
                .unwrap();
        }
        drop(root);
        let after = copied_history(&s, &path);
        let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
        assert_eq!(copied_history(&s, &path), after);
        assert_eq!(root.file_manifest().unwrap(), Some(expected));
        for ((id, key), session) in s.root.credentials.iter().zip(tokens) {
            root.with_access(id, key, session.access.expose(), 101, |_| ())
                .unwrap();
            let data = root
                .execute(id, key, "SELECT v FROM t WHERE id=1", &[])
                .unwrap();
            assert_eq!(data.results[0].rows, vec![vec![Value::Integer(123)]]);
        }
        drop(root);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        verify_source(&s);
    }
}
#[test]
fn v1_capture_and_backup_refuse_v2_before_staging_without_losing_any_file_bindings() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let before = copied_history(&s, &path);
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let out = t.path().join("forbidden");
    assert!(root.capture().is_err());
    assert!(capture_account_file_components(&mut root, &mut []).is_err());
    assert!(backup_account_file_components(&mut root, &mut [], &out).is_err());
    assert!(!out.exists());
    assert!(root.file_manifest().unwrap().is_some());
    drop(root);
    assert_eq!(copied_history(&s, &path), before);
    verify_source(&s);
    assert!(crate::capture_account_bundle_root(&path, pool()).is_err());
    assert!(crate::backup_account_bundle_root(&path, &out, pool()).is_err());
    assert!(!out.exists());
    assert!(AccountRoot::open_v2(&s.root.root, pool()).is_err());
    let mut original = AccountRoot::open(&s.root.root, pool()).unwrap();
    assert_eq!(original.file_manifest().unwrap(), None);
    original.capture().unwrap();
    drop(original);
    verify_source(&s);
}
#[test]
fn format_scope_roster_privacy_and_physical_corruption_refuse_without_repair_or_provisioning() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..10 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let id = &s.root.credentials[0].0;
            let files = path.join("files").join(id);
            let altered = match kind {
                0 => {
                    let m = manifest(&path);
                    let wrong =
                        FileRootBinding::new(id, "00000000000000000000000000000000").unwrap();
                    fs::write(
                        path.join("root.json"),
                        AccountFileRootManifest::new(
                            m.private_projects().to_vec(),
                            vec![wrong],
                            40,
                        )
                        .unwrap()
                        .to_bytes()
                        .unwrap(),
                    )
                    .unwrap();
                    path.join("root.json")
                }
                1 => {
                    fs::DirBuilder::new()
                        .mode(0o700)
                        .create(path.join("files/unexpected"))
                        .unwrap();
                    path.join("root.json")
                }
                2 => {
                    fs::set_permissions(path.join("files"), fs::Permissions::from_mode(0o755))
                        .unwrap();
                    path.join("root.json")
                }
                3 => {
                    fs::set_permissions(&files, fs::Permissions::from_mode(0o755)).unwrap();
                    path.join("root.json")
                }
                4 => {
                    let old = path.join("files");
                    let moved = t.path().join("original-files");
                    fs::rename(&old, &moved).unwrap();
                    symlink(moved, &old).unwrap();
                    path.join("root.json")
                }
                5 => {
                    let wal = files.join("metadata/redo.wal");
                    let mut b = fs::read(&wal).unwrap();
                    b[0] ^= 1;
                    fs::write(&wal, b).unwrap();
                    wal
                }
                6 => {
                    let payload = object(&files, true);
                    let mut b = fs::read(&payload).unwrap();
                    *b.last_mut().unwrap() ^= 1;
                    fs::write(&payload, b).unwrap();
                    payload
                }
                7 => {
                    let p = path.join("root.json");
                    fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
                    p
                }
                8 => {
                    let p = path.join("root.json");
                    fs::hard_link(&p, t.path().join("manifest-link")).unwrap();
                    p
                }
                _ => {
                    let p = path.join("root.json");
                    let saved = t.path().join("manifest");
                    fs::rename(&p, &saved).unwrap();
                    symlink(saved, &p).unwrap();
                    p
                }
            };
            let bytes = fs::read(&altered).unwrap();
            assert!(AccountRoot::open_v2(&path, pool()).is_err(), "kind {kind}");
            assert_eq!(fs::read(altered).unwrap(), bytes);
            assert!(File::open(&path).unwrap().try_lock().is_ok());
            verify_source(&s);
        }
    }
}
#[test]
fn cross_component_database_aliases_and_excessive_admission_refuse_before_acceptance() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let id = &s.root.credentials[0].0;
    let m = manifest(&path);
    let raw = fs::read(path.join("root.json")).unwrap();
    let mut ids = m.private_projects().to_vec();
    for i in 0..6 {
        let id = format!("{i:032x}");
        if !ids.contains(&id) && ids.len() < 5 {
            ids.push(id);
        }
    }
    ids.sort();
    let excessive = AccountFileRootManifest::new(ids, m.file_stores().to_vec(), 40)
        .unwrap()
        .to_bytes()
        .unwrap();
    fs::write(path.join("root.json"), &excessive).unwrap();
    let registry = ProjectStore::open_existing(path.join("registry")).unwrap();
    assert!(matches!(
        AccountRoot::open_v2(&path, pool()),
        Err(Error::Limit)
    ));
    drop(registry);
    assert_eq!(fs::read(path.join("root.json")).unwrap(), excessive);
    fs::write(path.join("root.json"), raw).unwrap();
    let wal = path.join("registry").join(id).join("data/redo.wal");
    let image = fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap();
    fs::write(&wal, &image).unwrap();
    assert!(matches!(
        AccountRoot::open_v2(&path, pool()),
        Err(Error::BundleRoot("duplicate component database identity"))
    ));
    assert_eq!(fs::read(wal).unwrap(), image);
    verify_source(&s);
}
#[test]
fn active_v2_readiness_refuses_same_byte_native_replacements_and_preserves_foreign_names() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in 0..6 {
        let t = tempfile::tempdir().unwrap();
        let (s, path) = copy(t.path(), 1, true);
        let root = AccountRoot::open_v2(&path, pool()).unwrap();
        let id = &s.root.credentials[0].0;
        let p = match kind {
            0 => path.join("files"),
            1 => path.join("files").join(id),
            2 => path.join("files").join(id).join("metadata"),
            3 => path.join("files").join(id).join("metadata/redo.wal"),
            4 => path
                .join("files")
                .join(id)
                .join("objects/.emilybase-objects"),
            _ => path.join("root.json"),
        };
        let saved = t.path().join("original");
        fs::rename(&p, &saved).unwrap();
        if saved.is_dir() {
            copy_directory(&saved, &p);
        } else {
            fs::copy(&saved, &p).unwrap();
        }
        assert!(root.projects().is_err());
        assert!(root.file_manifest().is_err());
        assert!(p.exists());
        assert!(saved.exists());
        drop(root);
        assert!(File::open(&path).unwrap().try_lock().is_ok());
        verify_source(&s);
    }
}
#[test]
fn opening_checks_late_component_and_manifest_changes_before_returning_original_owners() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for point in [
        "account_file_root_components_opened",
        "bundle_restore_inventory_validated",
        "account_file_root_service_opened",
    ] {
        for kind in 0..3 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, false);
            let id = &s.root.credentials[0].0;
            let changed = if kind == 0 {
                path.join("root.json")
            } else {
                path.join("files").join(id).join(if kind == 1 {
                    "metadata/redo.wal"
                } else {
                    "objects/.emilybase-objects"
                })
            };
            let selected = changed.clone();
            let saved = t.path().join("original");
            let _hook = durability::on_boundary(point, move || {
                fs::rename(&selected, &saved).unwrap();
                fs::copy(saved, selected).unwrap();
            });
            assert!(AccountRoot::open_v2(&path, pool()).is_err());
            assert!(changed.exists());
            assert!(File::open(&path).unwrap().try_lock().is_ok());
            verify_source(&s);
        }
    }
}
#[test]
fn retained_parent_movement_keeps_v2_data_account_and_file_owners_without_old_path_adoption() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let original = path.parent().unwrap().to_path_buf();
    let parent = File::open(&original).unwrap();
    let mut root = AccountRoot::open_v2_at(&parent, "v2", pool()).unwrap();
    drop(parent);
    let moved = t.path().join("moved");
    fs::rename(&original, &moved).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
    fs::write(original.join("keep"), b"foreign").unwrap();
    root.check_directory_at(&File::open(&moved).unwrap(), "v2")
        .unwrap();
    assert!(
        root.check_directory_at(&File::open(&original).unwrap(), "v2")
            .is_err()
    );
    let (id, key) = &s.root.credentials[0];
    root.sign_in(id, key, LOGIN, PASSWORD, 101).unwrap();
    root.execute(id, key, "UPDATE t SET v=99 WHERE id=1", &[])
        .unwrap();
    assert!(NativeFileRoot::open(moved.join("v2/files").join(id), id.parse().unwrap()).is_err());
    drop(root);
    AccountRoot::open_v2(moved.join("v2"), pool()).unwrap();
    assert_eq!(fs::read(original.join("keep")).unwrap(), b"foreign");
    verify_source(&s);
}
#[test]
fn actual_v2_open_kills_release_original_root_private_and_native_file_owners_without_reset() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, true)] {
        for point in [
            "account_file_root_components_opened",
            "bundle_restore_inventory_validated",
            "account_file_root_service_opened",
            "account_file_root_returned",
        ] {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), count, compact);
            let before = copied_history(&s, &path);
            let worker = Worker::start(&path, &path, "", "root-v2-open", point);
            worker.reach(point);
            assert!(File::open(&path).unwrap().try_lock().is_err());
            for (id, _) in &s.root.credentials {
                assert!(AccountStore::open(path.join("private").join(id), id, pool()).is_err());
                assert!(
                    NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap()).is_err()
                );
            }
            worker.kill();
            assert_eq!(copied_history(&s, &path), before);
            let root = AccountRoot::open_v2(&path, pool()).unwrap();
            assert!(root.file_manifest().unwrap().is_some());
            drop(root);
            assert_eq!(copied_history(&s, &path), before);
            verify_source(&s);
        }
    }
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_explicit_open_preserves_manifest_counts_and_v1_refusal(count in 0usize..3,compact in any::<bool>()) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let t=tempfile::tempdir().unwrap();let(s,path)=copy(t.path(),count,compact);let bytes=fs::read(path.join("root.json")).unwrap();let mut root=AccountRoot::open_v2(&path,pool()).unwrap();let m=root.file_manifest().unwrap().unwrap();prop_assert_eq!(m.private_projects().len(),count);prop_assert_eq!(m.file_stores().len(),count);prop_assert!(root.capture().is_err());drop(root);prop_assert_eq!(fs::read(path.join("root.json")).unwrap(),bytes);prop_assert!(AccountRoot::open(&path,pool()).is_err());verify_source(&s);
    }
}
