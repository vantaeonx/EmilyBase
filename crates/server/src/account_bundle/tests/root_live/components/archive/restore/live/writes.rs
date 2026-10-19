use super::*;
use crate::{ProjectFileUpload, Result};
const FRESH: FileId = FileId::from_bytes([31; 16]);
const NEW_OBJECT: ObjectId = ObjectId::from_bytes([32; 16]);
fn upload(payload: &[u8]) -> ProjectFileUpload<'_> {
    ProjectFileUpload {
        id: FRESH,
        object: NEW_OBJECT,
        owner: [33; 16],
        name: "fresh-name",
        payload,
    }
}
fn run(root: &mut AccountRoot, id: &str, key: &str, kind: u8) -> Result<()> {
    match kind {
        0 => root
            .publish_project_file(id, key, upload(b"fresh-payload"))
            .map(drop),
        1 => {
            let revision = root.project_file_info(id, key, FILE)?.unwrap().revision();
            root.rename_project_file(id, key, FILE, revision, "renamed")
                .map(drop)
        }
        2 => {
            let revision = root.project_file_info(id, key, FILE)?.unwrap().revision();
            root.remove_project_file(id, key, FILE, revision).map(drop)
        }
        _ => {
            let state = root.project_file_quota(id, key)?;
            root.set_project_file_quota(id, key, state, FileQuota::new(8, 32768)?)
                .map(drop)
        }
    }
}
fn phases(kind: u8) -> Vec<&'static str> {
    let mut points = vec!["account_file_write_before_operation"];
    if kind == 0 {
        points.push("account_file_write_before_blob");
    }
    points.extend([
        "account_file_write_before_commit",
        "account_file_write_committed",
        "account_file_write_verified",
        "account_file_write_native_returned",
    ]);
    points
}
fn assert_state(
    root: &mut AccountRoot,
    id: &str,
    key: &str,
    kind: u8,
    committed: bool,
    blob: bool,
) {
    let state = root.inspect_project_files(id, key).unwrap();
    assert_eq!(state.revision(), if committed { 4 } else { 3 });
    assert_eq!(state.usage().physical_objects, 2 + usize::from(blob));
    assert_eq!(
        state.usage().references,
        if kind == 0 && committed {
            2
        } else {
            usize::from(!(kind == 2 && committed))
        }
    );
    let original = root.project_file_info(id, key, FILE).unwrap();
    if kind == 2 && committed {
        assert!(original.is_none());
    } else {
        assert_eq!(
            original.unwrap().name(),
            if kind == 1 && committed {
                "renamed"
            } else {
                "synthetic-name"
            }
        );
    }
    assert_eq!(
        root.project_file_info(id, key, FRESH).unwrap().is_some(),
        kind == 0 && committed
    );
    assert_eq!(
        state.quota(),
        FileQuota::new(
            if kind == 3 && committed { 8 } else { 128 },
            if kind == 3 && committed { 32768 } else { 65536 }
        )
        .unwrap()
    );
}
#[test]
fn current_key_native_file_mutations_preserve_other_histories_and_survive_independent_rebackup_restore()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let (s, mut path) = copy(t.path(), 2, compact);
        let mut root = if compact {
            AccountRoot::open_v2_at(&File::open(path.parent().unwrap()).unwrap(), "v2", pool())
                .unwrap()
        } else {
            AccountRoot::open_v2(&path, pool()).unwrap()
        };
        if compact {
            let original = path.parent().unwrap().to_path_buf();
            let moved = t.path().join("moved-file-writer");
            fs::rename(&original, &moved).unwrap();
            fs::DirBuilder::new().mode(0o700).create(&original).unwrap();
            fs::write(original.join("keep"), b"foreign old parent").unwrap();
            path = moved.join("v2");
        }
        let (first, first_key) = &s.root.credentials[0];
        let (second, second_key) = &s.root.credentials[1];
        let foreign = root.project_file_quota(second, second_key).unwrap();
        assert!(matches!(
            root.set_project_file_quota(first, first_key, foreign, foreign.quota()),
            Err(Error::Files(emilybase_files::Error::Scope))
        ));
        assert_eq!(root.projects().unwrap().len(), 2);
        let debug = format!("{:?}", upload(b"synthetic private payload"));
        assert!(!debug.contains("fresh-name"));
        assert!(!debug.contains("synthetic private payload"));
        let before = s
            .root
            .credentials
            .iter()
            .flat_map(|(id, _)| {
                [
                    fs::read(path.join("private").join(id).join("redo.wal")).unwrap(),
                    fs::read(path.join("registry").join(id).join("data/redo.wal")).unwrap(),
                    fs::read(path.join("registry").join(id).join("project.json")).unwrap(),
                ]
            })
            .collect::<Vec<_>>();
        for (id, key) in &s.root.credentials {
            let created = root
                .publish_project_file(id, key, upload(b"fresh-payload"))
                .unwrap();
            assert_eq!(created.revision(), 4);
            assert_eq!(
                root.read_project_file(id, key, FRESH, 13)
                    .unwrap()
                    .payload(),
                b"fresh-payload"
            );
            let renamed = root
                .rename_project_file(id, key, FRESH, created.revision(), "renamed")
                .unwrap();
            assert_eq!(renamed.revision(), 5);
            root.remove_project_file(id, key, FRESH, renamed.revision())
                .unwrap();
            assert!(root.project_file_info(id, key, FRESH).unwrap().is_none());
            let old = root.project_file_quota(id, key).unwrap();
            let new = root
                .set_project_file_quota(id, key, old, FileQuota::new(8, 32768).unwrap())
                .unwrap();
            assert_eq!(new.revision(), 7);
            let state = root.inspect_project_files(id, key).unwrap();
            assert_eq!(state.usage().orphans, 2);
            assert_eq!(state.usage().physical_objects, 3);
            assert_eq!(state.usage().references, 1);
        }
        let after = s
            .root
            .credentials
            .iter()
            .flat_map(|(id, _)| {
                [
                    fs::read(path.join("private").join(id).join("redo.wal")).unwrap(),
                    fs::read(path.join("registry").join(id).join("data/redo.wal")).unwrap(),
                    fs::read(path.join("registry").join(id).join("project.json")).unwrap(),
                ]
            })
            .collect::<Vec<_>>();
        assert_eq!(before, after);
        let archive = t.path().join("checked-archive");
        crate::backup_account_file_root_v2(&mut root, &archive).unwrap();
        drop(root);
        assert!(crate::restore_account_file_archive(&archive, &s.root.root, pool(), 80).is_err());
        let target = t.path().join("independent-file-copy");
        crate::restore_account_file_archive(&archive, &target, pool(), 80).unwrap();
        let mut restored = AccountRoot::open_v2(target, pool()).unwrap();
        for (id, key) in &s.root.credentials {
            assert_eq!(
                restored.inspect_project_files(id, key).unwrap().revision(),
                7
            );
            assert_eq!(
                restored.project_file_quota(id, key).unwrap().quota(),
                FileQuota::new(8, 32768).unwrap()
            );
            assert_eq!(
                restored
                    .read_project_file(id, key, FILE, 17)
                    .unwrap()
                    .payload(),
                b"synthetic-payload"
            );
        }
        drop(restored);
        if compact {
            assert_eq!(
                fs::read(t.path().join("output/keep")).unwrap(),
                b"foreign old parent"
            );
        }
        verify_source(&s);
    }
}
#[test]
fn denied_missing_foreign_and_rotated_keys_never_reach_native_write_admission() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 2, false);
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let (id, key) = &s.root.credentials[0];
    let state = root.project_file_quota(id, key).unwrap();
    let before = copied_history(&s, &path);
    let _hook = durability::on_boundary("account_file_write_before_operation", || {
        panic!("denied request reached native write")
    });
    for denied in ["malformed-key", s.root.credentials[1].1.as_str()] {
        assert!(matches!(
            root.publish_project_file(id, denied, upload(b"payload")),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.rename_project_file(id, denied, FILE, 3, "new"),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.remove_project_file(id, denied, FILE, 3),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.set_project_file_quota(id, denied, state, state.quota()),
            Err(Error::Denied)
        ));
        assert!(matches!(
            root.project_file_quota(id, denied),
            Err(Error::Denied)
        ));
    }
    assert!(matches!(
        root.publish_project_file("unknown", key, upload(b"payload")),
        Err(Error::Denied)
    ));
    assert_eq!(copied_history(&s, &path), before);
    let rotated = root.rotate_project_key(id).unwrap();
    assert!(matches!(
        root.publish_project_file(id, key, upload(b"payload")),
        Err(Error::Denied)
    ));
    drop(_hook);
    root.publish_project_file(id, &rotated.api_key, upload(b"payload"))
        .unwrap();
    drop(root);
    let mut v1 = AccountRoot::open(&s.root.root, pool()).unwrap();
    assert!(matches!(
        v1.publish_project_file(id, key, upload(b"payload")),
        Err(Error::Denied)
    ));
    assert!(matches!(
        v1.rename_project_file(id, key, FILE, 3, "new"),
        Err(Error::Denied)
    ));
    assert!(matches!(
        v1.remove_project_file(id, key, FILE, 3),
        Err(Error::Denied)
    ));
    assert!(matches!(v1.project_file_quota(id, key), Err(Error::Denied)));
    drop(v1);
    verify_source(&s);
}
#[test]
fn native_noops_and_invalid_cas_inputs_keep_root_healthy_without_new_wal_or_objects() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, true);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let before = copied_history(&s, &path);
    let quota = root.project_file_quota(id, key).unwrap();
    let _hook = durability::on_boundary("account_file_write_before_commit", || {
        panic!("noop/input error attempted a commit")
    });
    root.rename_project_file(id, key, FILE, 3, "synthetic-name")
        .unwrap();
    root.set_project_file_quota(id, key, quota, quota.quota())
        .unwrap();
    assert!(matches!(
        root.rename_project_file(id, key, FILE, 2, "renamed"),
        Err(Error::Files(emilybase_files::Error::Conflict))
    ));
    assert!(matches!(
        root.remove_project_file(id, key, FRESH, 3),
        Err(Error::Files(emilybase_files::Error::Missing))
    ));
    let mut invalid = upload(b"payload");
    invalid.name = "\n";
    assert!(matches!(
        root.publish_project_file(id, key, invalid),
        Err(Error::Files(emilybase_files::Error::Name))
    ));
    assert!(
        root.set_project_file_quota(id, key, quota, FileQuota::new(1, 1).unwrap())
            .is_err()
    );
    assert_eq!(copied_history(&s, &path), before);
    assert_eq!(root.projects().unwrap().len(), 1);
    drop(root);
    verify_source(&s);
}
#[test]
fn original_key_common_native_and_payload_descriptors_span_actual_mutation_phases() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for kind in 0..4 {
        for point in phases(kind) {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, kind % 2 == 0);
            let (id, key) = &s.root.credentials[0];
            let original = path.clone();
            let project = id.clone();
            let _hook = durability::on_boundary(point, move || {
                assert!(File::open(&original).unwrap().try_lock().is_err());
                assert!(ProjectStore::open_existing(original.join("registry")).is_err());
                assert!(
                    AccountStore::open(original.join("private").join(&project), &project, pool())
                        .is_err()
                );
                let native = original.join("files").join(&project);
                assert!(NativeFileRoot::open(&native, project.parse().unwrap()).is_err());
                assert!(
                    fds(
                        std::process::id(),
                        &original
                            .join("registry")
                            .join(&project)
                            .join("project.json")
                    ) > 0
                );
                assert!(
                    Database::open(original.join("registry").join(&project).join("data")).is_ok()
                );
                if matches!(
                    point,
                    "account_file_write_before_commit"
                        | "account_file_write_committed"
                        | "account_file_write_verified"
                ) {
                    let payload = if kind == 0 {
                        native.join("objects").join(format!("{NEW_OBJECT}.object"))
                    } else {
                        object(&native, false)
                    };
                    assert!(fds(std::process::id(), &payload) > 0);
                    if kind == 3 {
                        assert!(fds(std::process::id(), &object(&native, true)) > 0);
                    }
                }
            });
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            run(&mut root, id, key, kind).unwrap();
            assert_state(&mut root, id, key, kind, true, kind == 0);
            drop(root);
            verify_source(&s);
        }
    }
}
#[test]
fn late_common_or_key_failure_preserves_orphans_commits_and_poisons_entire_root_after_publication()
{
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..4 {
            for point in phases(kind) {
                for key_metadata in [false, true] {
                    let t = tempfile::tempdir().unwrap();
                    let (s, path) = copy(t.path(), 1, compact);
                    let (id, key) = &s.root.credentials[0];
                    let changed = if key_metadata {
                        path.join("registry").join(id).join("project.json")
                    } else {
                        path.join("root.json")
                    };
                    let selected = changed.clone();
                    let saved = t.path().join("original-metadata");
                    let retained = saved.clone();
                    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                    let before =
                        fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap();
                    let _hook = durability::on_boundary(point, move || {
                        fs::rename(&selected, &retained).unwrap();
                        fs::copy(retained, selected).unwrap();
                    });
                    let result = run(&mut root, id, key, kind);
                    assert!(result.is_err(), "{kind} {point}");
                    drop(_hook);
                    fs::rename(saved, changed).unwrap();
                    let committed = matches!(
                        point,
                        "account_file_write_committed"
                            | "account_file_write_verified"
                            | "account_file_write_native_returned"
                    );
                    let blob = kind == 0
                        && !matches!(
                            point,
                            "account_file_write_before_operation"
                                | "account_file_write_before_blob"
                        );
                    let unknown = committed || blob;
                    assert_eq!(
                        matches!(result, Err(Error::AccountFileWriteUnknown(_))),
                        unknown,
                        "{kind} {point}"
                    );
                    if unknown {
                        assert!(matches!(
                            root.projects(),
                            Err(Error::AccountFileWritePoisoned)
                        ));
                        assert!(matches!(
                            root.read_project_file(id, key, FILE, 17),
                            Err(Error::AccountFileWritePoisoned)
                        ));
                        assert!(matches!(
                            root.execute(id, key, "SELECT * FROM t", &[]),
                            Err(Error::AccountFileWritePoisoned)
                        ));
                        assert!(matches!(
                            root.capture_v2(),
                            Err(Error::AccountFileWritePoisoned)
                        ));
                        assert!(matches!(
                            root.rotate_project_key(id),
                            Err(Error::AccountFileWritePoisoned)
                        ));
                    } else if point == "account_file_write_before_operation"
                        || point == "account_file_write_before_blob"
                    {
                        assert_eq!(root.projects().unwrap().len(), 1);
                    }
                    if !committed {
                        assert_eq!(
                            fs::read(path.join("files").join(id).join("metadata/redo.wal"))
                                .unwrap(),
                            before
                        );
                    }
                    drop(root);
                    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                    assert_state(&mut root, id, key, kind, committed, blob);
                    drop(root);
                    verify_source(&s);
                }
            }
        }
    }
}
#[test]
fn noop_late_common_refusal_does_not_invent_publication_and_restored_original_selection_stays_usable()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    let t = tempfile::tempdir().unwrap();
    let (s, path) = copy(t.path(), 1, false);
    let (id, key) = &s.root.credentials[0];
    let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
    let before = copied_history(&s, &path);
    for kind in 0..2 {
        let current = root.project_file_quota(id, key).unwrap();
        let selected = path.join("root.json");
        let changed = selected.clone();
        let saved = t.path().join("original");
        let retained = saved.clone();
        let _hook = durability::on_boundary("account_file_write_native_returned", move || {
            fs::rename(&selected, &retained).unwrap();
            fs::copy(retained, selected).unwrap();
        });
        let result = if kind == 0 {
            root.rename_project_file(id, key, FILE, 3, "synthetic-name")
                .map(drop)
        } else {
            root.set_project_file_quota(id, key, current, current.quota())
                .map(drop)
        };
        assert!(result.is_err());
        assert!(!matches!(result, Err(Error::AccountFileWriteUnknown(_))));
        drop(_hook);
        fs::rename(saved, changed).unwrap();
        assert_eq!(root.projects().unwrap().len(), 1);
        assert_eq!(copied_history(&s, &path), before);
    }
    drop(root);
    verify_source(&s);
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn generated_native_mutation_recovery_matches_scoped_visibility_and_physical_charge(compact in any::<bool>(),kind in 0u8..4) {
        let _serial=durability::PROCESS_TESTS.blocking_lock();let t=tempfile::tempdir().unwrap();let(s,path)=copy(t.path(),1,compact);let(id,key)=&s.root.credentials[0];let mut root=AccountRoot::open_v2(&path,pool()).unwrap();run(&mut root,id,key,kind).unwrap();drop(root);let mut root=AccountRoot::open_v2(path,pool()).unwrap();assert_state(&mut root,id,key,kind,true,kind==0);drop(root);verify_source(&s);
    }
}

#[test]
fn native_postcommit_failure_is_unknown_at_common_root_and_preserves_the_foreign_namespace() {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..4 {
            let t = tempfile::tempdir().unwrap();
            let (s, path) = copy(t.path(), 1, compact);
            let (id, key) = &s.root.credentials[0];
            let native = path.join("files").join(id);
            let foreign = native.join("foreign");
            let changed = foreign.clone();
            let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
            let _hook = durability::on_boundary("account_file_write_committed", move || {
                fs::write(&changed, b"preserve foreign bytes").unwrap()
            });
            assert!(matches!(
                run(&mut root, id, key, kind),
                Err(Error::AccountFileWriteUnknown(_))
            ));
            assert!(matches!(
                root.projects(),
                Err(Error::AccountFileWritePoisoned)
            ));
            drop(_hook);
            drop(root);
            assert_eq!(fs::read(&foreign).unwrap(), b"preserve foreign bytes");
            assert!(NativeFileRoot::open(&native, id.parse().unwrap()).is_err());
            let database = Database::open(native.join("metadata")).unwrap();
            assert_eq!(database.last_transaction(), 4);
            drop(database);
            verify_source(&s);
        }
    }
}
#[test]
fn actual_common_file_writer_kills_recover_original_binding_confirmed_state_and_next_independent_write()
 {
    let _serial = durability::PROCESS_TESTS.blocking_lock();
    for compact in [false, true] {
        for kind in 0..4 {
            let mut points = phases(kind);
            points.push("account_file_write_returned");
            for point in points {
                let t = tempfile::tempdir().unwrap();
                let (s, path) = copy(t.path(), 1, compact);
                let (id, key) = &s.root.credentials[0];
                let key_file = t.path().join("service-key");
                fs::write(&key_file, key.as_bytes()).unwrap();
                fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
                let original = fs::read(path.join("root.json")).unwrap();
                let before =
                    fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap();
                let worker = Worker::start(
                    &path,
                    &key_file,
                    id,
                    &format!("root-v2-file-write-{kind}"),
                    point,
                );
                worker.reach(point);
                assert!(File::open(&path).unwrap().try_lock().is_err());
                assert!(
                    NativeFileRoot::open(path.join("files").join(id), id.parse().unwrap()).is_err()
                );
                worker.kill();
                assert_eq!(fs::read(path.join("root.json")).unwrap(), original);
                let committed = matches!(
                    point,
                    "account_file_write_committed"
                        | "account_file_write_verified"
                        | "account_file_write_native_returned"
                        | "account_file_write_returned"
                );
                let blob = kind == 0
                    && !matches!(
                        point,
                        "account_file_write_before_operation" | "account_file_write_before_blob"
                    );
                if !committed {
                    assert_eq!(
                        fs::read(path.join("files").join(id).join("metadata/redo.wal")).unwrap(),
                        before
                    );
                }
                let mut root = AccountRoot::open_v2(&path, pool()).unwrap();
                assert_state(&mut root, id, key, kind, committed, blob);
                root.publish_project_file(
                    id,
                    key,
                    ProjectFileUpload {
                        id: FileId::from_bytes([34; 16]),
                        object: ObjectId::from_bytes([35; 16]),
                        owner: [33; 16],
                        name: "next",
                        payload: b"next",
                    },
                )
                .unwrap();
                drop(root);
                verify_source(&s);
            }
        }
    }
}
