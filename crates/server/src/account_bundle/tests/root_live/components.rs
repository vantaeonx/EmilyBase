use super::*;
use crate::{capture_account_file_components, component_snapshot};
use emilybase_files::{
    FileArchiveReader, FileId, FileQuota, NativeFileRoot, initialize_file_root,
    restore_file_archive,
};
use emilybase_object_storage::{ObjectId, ProjectDirectory, ProjectId};
use std::io::Read;
use std::os::unix::fs::MetadataExt;

const FILE: FileId = FileId::from_bytes([2; 16]);
const BLOB: ObjectId = ObjectId::from_bytes([3; 16]);
const ORPHAN: ObjectId = ObjectId::from_bytes([5; 16]);
fn prepare(parent: &Path, count: usize, compact: bool) -> (Restored, PathBuf, Vec<PathBuf>) {
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    let f = restored(parent, count, compact);
    let directory = parent.join("native-files");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let mut ids = f
        .credentials
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    let mut paths = Vec::new();
    for id in ids {
        let path = directory.join(&id);
        let project: ProjectId = id.parse().unwrap();
        initialize_file_root(&path, project, FileQuota::new(128, 65536).unwrap()).unwrap();
        let mut root = NativeFileRoot::open(&path, project).unwrap();
        root.publish(FILE, BLOB, [4; 16], "synthetic-name", b"synthetic-payload")
            .unwrap();
        drop(root);
        ProjectDirectory::open(path.join("objects"), project)
            .unwrap()
            .put(ORPHAN, b"orphan")
            .unwrap();
        if compact {
            Database::open(path.join("metadata"))
                .unwrap()
                .compact()
                .unwrap();
        }
        paths.push(path);
    }
    (f, directory, paths)
}
fn open(paths: &[PathBuf]) -> Vec<NativeFileRoot> {
    paths
        .iter()
        .map(|p| {
            NativeFileRoot::open(p, p.file_name().unwrap().to_str().unwrap().parse().unwrap())
                .unwrap()
        })
        .collect()
}
fn history(f: &Restored, paths: &[PathBuf]) -> Vec<Vec<u8>> {
    let mut images = histories(f);
    for path in paths {
        images.push(fs::read(path.join("metadata/redo.wal")).unwrap());
        let mut entries = fs::read_dir(path.join("objects"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        images.extend(entries.iter().map(|p| fs::read(p).unwrap()));
    }
    images
}
fn object(path: &Path, orphan: bool) -> PathBuf {
    path.join("objects")
        .join(format!("{}.object", if orphan { ORPHAN } else { BLOB }))
}
fn fds(pid: u32, path: &Path) -> usize {
    let expected = fs::metadata(path).unwrap();
    fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|e| fs::metadata(e.ok()?.path()).ok())
        .filter(|m| (m.dev(), m.ino()) == (expected.dev(), expected.ino()))
        .count()
}

#[test]
fn exact_mixed_copies_hold_every_original_owner_and_charge_orphans_without_source_descriptors() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let temp = tempfile::tempdir().unwrap();
        let (f, _, paths) = prepare(temp.path(), count, compact);
        let before = history(&f, &paths);
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();
        let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
        let mut files = open(&paths);
        let expected = accounts.capture().unwrap().into_bytes();
        let snapshot =
            component_snapshot::capture_with(&mut accounts, &mut files, &mut |snapshot| {
                assert_eq!(snapshot.files().len(), count);
                assert!(File::open(&f.root).unwrap().try_lock().is_err());
                assert!(ProjectStore::open_existing(f.root.join("registry")).is_err());
                for path in &paths {
                    let id = path.file_name().unwrap().to_str().unwrap();
                    assert!(Database::open(f.root.join("registry").join(id).join("data")).is_err());
                    assert!(
                        AccountStore::open(f.root.join("private").join(id), id, pool()).is_err()
                    );
                    assert!(Database::open(path.join("metadata")).is_err());
                    assert!(
                        ProjectDirectory::open(path.join("objects"), id.parse().unwrap()).is_err()
                    );
                    for orphan in [false, true] {
                        assert!(fds(std::process::id(), &object(path, orphan)) >= 1);
                    }
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(snapshot.accounts().bundle_bytes(), expected);
        assert_eq!(snapshot.physical_objects(), count * 2);
        assert_eq!(snapshot.manifest().reset_at(), 50);
        assert_eq!(
            snapshot.manifest().private_projects(),
            accounts.snapshot_manifest().unwrap().private_projects
        );
        assert_eq!(
            crate::inspect_account_file_root_manifest_bytes(snapshot.manifest_bytes()).unwrap(),
            *snapshot.manifest()
        );
        let charged = snapshot.accounts().bundle_bytes().len()
            + snapshot.manifest_bytes().len()
            + snapshot
                .files()
                .iter()
                .map(|s| {
                    s.metadata_bytes().len()
                        + s.objects()
                            .objects()
                            .iter()
                            .map(|o| o.payload().len() + emilybase_object_storage::HEADER_BYTES)
                            .sum::<usize>()
                })
                .sum::<usize>();
        assert_eq!(snapshot.copied_bytes(), charged);
        assert!(!format!("{snapshot:?}").contains("synthetic"));
        assert_eq!(history(&f, &paths), before);
        for path in &paths {
            for orphan in [false, true] {
                assert_eq!(fds(std::process::id(), &object(path, orphan)), 0);
            }
        }
        drop(files);
        drop(accounts);
        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
        assert_eq!(snapshot.files().len(), count);
    }
}

#[test]
fn malformed_component_order_membership_and_count_refuse_before_account_capture() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, directory, paths) = prepare(temp.path(), 2, false);
    let foreign = directory.join("ffffffffffffffffffffffffffffffff");
    initialize_file_root(
        &foreign,
        ProjectId::from_bytes([255; 16]),
        FileQuota::new(0, 0).unwrap(),
    )
    .unwrap();
    let before = history(&f, &paths);
    let mut account = AccountRoot::open(&f.root, pool()).unwrap();
    for list in [
        vec![paths[1].clone(), paths[0].clone()],
        vec![foreign.clone()],
    ] {
        let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = reached.clone();
        let _guard = durability::on_boundary("root_capture_owners_locked", move || {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let mut files = open(&list);
        assert!(capture_account_file_components(&mut account, &mut files).is_err());
        assert!(!reached.load(std::sync::atomic::Ordering::SeqCst));
        for file in &mut files {
            file.inspect().unwrap();
        }
    }
    // Distinct physical roots carrying one project are still duplicate scopes.
    let mut files = open(&paths[..1]);
    let duplicate = directory.join("duplicate");
    let mut image = Vec::new();
    FileArchiveReader::from_snapshot(&files[0].capture().unwrap())
        .unwrap()
        .read_to_end(&mut image)
        .unwrap();
    restore_file_archive(&image, files[0].inspect().unwrap().project(), &duplicate).unwrap();
    let project = files[0].inspect().unwrap().project();
    files.push(NativeFileRoot::open(&duplicate, project).unwrap());
    assert!(capture_account_file_components(&mut account, &mut files).is_err());
    for n in 2..5 {
        let duplicate = directory.join(format!("duplicate-{n}"));
        restore_file_archive(&image, project, &duplicate).unwrap();
        files.push(NativeFileRoot::open(&duplicate, project).unwrap());
    }
    let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = reached.clone();
    let guard = durability::on_boundary("root_capture_owners_locked", move || {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    assert!(matches!(
        capture_account_file_components(&mut account, &mut files),
        Err(Error::Limit)
    ));
    assert!(!reached.load(std::sync::atomic::Ordering::SeqCst));
    drop(guard);
    drop(files);
    let mut files = open(&paths);
    assert_eq!(
        capture_account_file_components(&mut account, &mut files[..1])
            .unwrap()
            .files()
            .len(),
        1
    );
    assert_eq!(history(&f, &paths), before);
}

#[test]
fn aggregate_physical_cap_includes_unreferenced_objects_before_capture_readers() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, _, paths) = prepare(temp.path(), 2, false);
    for (index, path) in paths.iter().enumerate() {
        let project = path.file_name().unwrap().to_str().unwrap().parse().unwrap();
        let mut objects = ProjectDirectory::open(path.join("objects"), project).unwrap();
        for n in 10..72 {
            objects.put(ObjectId::from_bytes([n; 16]), b"").unwrap();
        }
        if index == 1 {
            objects.put(ObjectId::from_bytes([80; 16]), b"").unwrap();
        }
    }
    let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = reached.clone();
    let guard = durability::on_boundary("root_capture_owners_locked", move || {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    assert!(matches!(
        capture_account_file_components(&mut accounts, &mut files),
        Err(Error::Limit)
    ));
    assert!(!reached.load(std::sync::atomic::Ordering::SeqCst));
    drop(guard);
    assert_eq!(files[0].inspect().unwrap().usage().physical_objects, 64);
    assert_eq!(files[1].inspect().unwrap().usage().physical_objects, 65);
    drop(files);
    fs::remove_file(paths[1].join(format!("objects/{}.object", ObjectId::from_bytes([80; 16]))))
        .unwrap();
    let mut files = open(&paths);
    let snapshot = capture_account_file_components(&mut accounts, &mut files).unwrap();
    assert_eq!(snapshot.physical_objects(), 128);
    assert_eq!(
        snapshot
            .files()
            .iter()
            .map(|s| s.files().len())
            .sum::<usize>(),
        2
    );
}

#[test]
fn later_source_substitution_withholds_all_copies_and_only_changed_file_owner_is_poisoned() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for changed in [0, 1] {
            for orphan in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let (f, _, paths) = prepare(temp.path(), 2, compact);
                let before = history(&f, &paths);
                let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
                let mut files = open(&paths);
                let selected = object(&paths[changed], orphan);
                let saved = temp.path().join("saved");
                let outcome =
                    component_snapshot::capture_with(&mut accounts, &mut files, &mut |_| {
                        fs::rename(&selected, &saved).unwrap();
                        fs::copy(&saved, &selected).unwrap();
                        Ok(())
                    });
                assert!(outcome.is_err());
                assert!(matches!(
                    files[changed].inspect(),
                    Err(emilybase_files::Error::Poisoned)
                ));
                files[1 - changed].inspect().unwrap();
                accounts.projects().unwrap();
                assert_eq!(history(&f, &paths), before);
                assert_eq!(fs::read(selected).unwrap(), fs::read(saved).unwrap());
            }
        }
    }
}

#[test]
fn account_wal_substitution_during_file_work_withholds_joint_result_without_poisoning_files() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for private in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (f, _, paths) = prepare(temp.path(), 1, false);
        let (id, _) = &f.credentials[0];
        let wal = if private {
            f.root.join("private").join(id).join("redo.wal")
        } else {
            f.root.join("registry").join(id).join("data/redo.wal")
        };
        let before = history(&f, &paths);
        let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
        let mut files = open(&paths);
        let outcome = component_snapshot::capture_with(&mut accounts, &mut files, &mut |_| {
            let saved = temp.path().join("saved");
            fs::rename(&wal, &saved).unwrap();
            fs::copy(&saved, &wal).unwrap();
            Ok(())
        });
        assert!(outcome.is_err());
        files[0].inspect().unwrap();
        assert_eq!(history(&f, &paths), before);
    }
}

#[test]
fn file_namespace_history_mode_and_content_changes_withhold_joint_results_without_repair() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in 0..8 {
        let temp = tempfile::tempdir().unwrap();
        let (f, _, paths) = prepare(temp.path(), 1, false);
        let path = &paths[0];
        let before = histories(&f);
        let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
        let mut files = open(&paths);
        let saved = temp.path().join("saved");
        let outcome = component_snapshot::capture_with(&mut accounts, &mut files, &mut |_| {
            match kind {
                0 => {
                    let wal = path.join("metadata/redo.wal");
                    fs::rename(&wal, &saved).unwrap();
                    fs::copy(saved.clone(), wal).unwrap();
                }
                1..=3 => {
                    let selected = match kind {
                        1 => path.join("metadata"),
                        2 => path.join("objects"),
                        _ => path.clone(),
                    };
                    fs::rename(&selected, &saved).unwrap();
                    super::at::copy_directory(&saved, &selected);
                }
                4 => {
                    let marker = path.join("objects/.emilybase-objects");
                    fs::rename(&marker, &saved).unwrap();
                    fs::copy(&saved, marker).unwrap();
                }
                5 => fs::hard_link(object(path, true), &saved).unwrap(),
                6 => fs::set_permissions(object(path, false), fs::Permissions::from_mode(0o644))
                    .unwrap(),
                _ => {
                    let selected = object(path, true);
                    let mut bytes = fs::read(&selected).unwrap();
                    *bytes.last_mut().unwrap() ^= 1;
                    fs::write(selected, bytes).unwrap();
                }
            }
            Ok(())
        });
        assert!(outcome.is_err(), "kind={kind}");
        assert!(matches!(
            files[0].inspect(),
            Err(emilybase_files::Error::Poisoned)
        ));
        accounts.projects().unwrap();
        assert_eq!(histories(&f), before);
        drop(files);
        if kind < 6 {
            assert!(saved.exists());
        }
        if kind == 6 {
            assert_eq!(
                fs::metadata(object(path, false))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }
        if kind == 7 {
            assert!(
                NativeFileRoot::open(
                    path,
                    path.file_name().unwrap().to_str().unwrap().parse().unwrap()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn callback_error_keeps_healthy_sources_and_copies_restore_independently_after_mutation() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let temp = tempfile::tempdir().unwrap();
    let (f, _, paths) = prepare(temp.path(), 1, true);
    let (id, key) = &f.credentials[0];
    let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
    let mut files = open(&paths);
    let session = accounts.sign_in(id, key, LOGIN, PASSWORD, 100).unwrap();
    let before = history(&f, &paths);
    assert!(matches!(
        component_snapshot::capture_with(&mut accounts, &mut files, &mut |_| Err(Error::Limit)),
        Err(Error::Limit)
    ));
    files[0].inspect().unwrap();
    accounts.projects().unwrap();
    let snapshot = capture_account_file_components(&mut accounts, &mut files).unwrap();
    assert_eq!(history(&f, &paths), before);
    let exact = snapshot.copied_bytes();
    assert_eq!(
        component_snapshot::capture_with_limit(&mut accounts, &mut files, &mut |_| Ok(()), exact)
            .unwrap()
            .copied_bytes(),
        exact
    );
    for limit in [0, snapshot.accounts().bundle_bytes().len(), exact - 1] {
        let mut called = false;
        assert!(matches!(
            component_snapshot::capture_with_limit(
                &mut accounts,
                &mut files,
                &mut |_| {
                    called = true;
                    Ok(())
                },
                limit
            ),
            Err(Error::Limit)
        ));
        assert!(!called);
        files[0].inspect().unwrap();
        accounts.projects().unwrap();
    }
    assert_eq!(history(&f, &paths), before);
    accounts
        .execute(id, key, "INSERT INTO t VALUES(2,42)", &[])
        .unwrap();
    files[0]
        .publish(
            FileId::from_bytes([6; 16]),
            ObjectId::from_bytes([7; 16]),
            [4; 16],
            "later",
            b"later",
        )
        .unwrap();
    accounts
        .with_access(id, key, session.access.expose(), 100, |_| ())
        .unwrap();
    let copy = temp.path().join("account-copy");
    restore_account_bundle_bytes(snapshot.accounts().bundle_bytes(), &copy, pool(), 200).unwrap();
    let mut restored = AccountRoot::open(&copy, pool()).unwrap();
    assert_eq!(
        restored
            .execute(id, key, "SELECT * FROM t", &[])
            .unwrap()
            .results[0]
            .rows
            .len(),
        1
    );
    assert!(
        restored
            .with_access(id, key, session.access.expose(), 200, |_| ())
            .is_err()
    );
    let mut image = Vec::new();
    FileArchiveReader::from_snapshot(&snapshot.files()[0])
        .unwrap()
        .read_to_end(&mut image)
        .unwrap();
    let file_copy = temp.path().join("file-copy");
    restore_file_archive(&image, id.parse().unwrap(), &file_copy).unwrap();
    let mut restored_files = NativeFileRoot::open(&file_copy, id.parse().unwrap()).unwrap();
    assert_eq!(restored_files.list().unwrap().len(), 1);
    restored_files
        .publish(
            FileId::from_bytes([8; 16]),
            ObjectId::from_bytes([9; 16]),
            [4; 16],
            "copy-only",
            b"copy",
        )
        .unwrap();
    assert_eq!(files[0].list().unwrap().len(), 2);
    assert_eq!(snapshot.files()[0].objects().objects().len(), 2);
}

#[test]
fn killed_collectors_release_original_owners_and_preserve_exact_sources_at_each_boundary() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for (count, compact) in [(0, false), (1, false), (2, true)] {
        for point in [
            "components_admitted",
            "components_account_copied",
            "components_retained",
            "components_ack",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (f, directory, paths) = prepare(temp.path(), count, compact);
            let before = history(&f, &paths);
            let worker = Worker::start(&f.root, &directory, "", "components-capture", point);
            worker.reach(point);
            assert!(File::open(&f.root).unwrap().try_lock().is_err());
            for path in &paths {
                assert!(Database::open(path.join("metadata")).is_err());
                for orphan in [false, true] {
                    assert_eq!(
                        fds(worker.pid(), &object(path, orphan)) > 0,
                        point == "components_retained"
                    );
                }
            }
            worker.kill();
            assert_eq!(history(&f, &paths), before);
            let mut accounts = AccountRoot::open(&f.root, pool()).unwrap();
            let mut files = open(&paths);
            assert_eq!(
                capture_account_file_components(&mut accounts, &mut files)
                    .unwrap()
                    .files()
                    .len(),
                count
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_joint_copies_match_independent_rows_and_file_revisions(compact in any::<bool>(), values in prop::collection::vec(-100i64..101,1..5)) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let temp=tempfile::tempdir().unwrap();let(f,_,paths)=prepare(temp.path(),1,compact);let(id,key)=&f.credentials[0];let mut accounts=AccountRoot::open(&f.root,pool()).unwrap();let mut files=open(&paths);let mut expected=vec![vec![Value::Integer(1),Value::Integer(0)]];
        for(step,value)in values.into_iter().enumerate(){let n=step as i64+2;accounts.execute(id,key,"INSERT INTO t VALUES($1,$2)",&[Value::Integer(n),Value::Integer(value)]).unwrap();expected.push(vec![Value::Integer(n),Value::Integer(value)]);let info=files[0].list().unwrap().remove(0);files[0].rename(FILE,info.revision(),&format!("revision-{step}")).unwrap();let before=history(&f,&paths);let snapshot=capture_account_file_components(&mut accounts,&mut files).unwrap();prop_assert_eq!(history(&f,&paths),before);prop_assert_eq!(snapshot.files()[0].files()[0].name(),format!("revision-{step}"));let copy=temp.path().join(format!("copy-{step}"));restore_account_bundle_bytes(snapshot.accounts().bundle_bytes(),&copy,pool(),200).unwrap();let restored=AccountRoot::open(&copy,pool()).unwrap();prop_assert_eq!(&restored.execute(id,key,"SELECT id,v FROM t ORDER BY id",&[]).unwrap().results[0].rows,&expected);}
    }
}
