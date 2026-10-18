use super::*;
use std::os::unix::fs::DirBuilderExt;

fn prepare(parent: &Path, empty: bool, compact: bool) -> (Vec<u8>, Option<String>, PathBuf) {
    let (_, bytes, id) = fixture(parent, empty, compact);
    let directory = parent.join("file-parent");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    if let Some(id) = &id {
        fs::rename(parent.join("files"), directory.join(id)).unwrap();
    }
    (bytes, id, directory)
}
fn command(
    parent: &Path,
    target: &Path,
    file_parent: Option<&Path>,
    projects: &[&str],
    stdout: Option<File>,
) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_emilybase"));
    cmd.current_dir(parent)
        .arg("account-file-backup")
        .arg("root")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(stdout.map(Stdio::from).unwrap_or_else(Stdio::piped))
        .stderr(Stdio::piped());
    if let Some(path) = file_parent {
        cmd.arg("--file-parent").arg(path);
    }
    for project in projects {
        cmd.arg("--file-project").arg(project);
    }
    cmd.output().unwrap()
}
fn histories(parent: &Path, id: Option<&str>) -> Vec<Vec<u8>> {
    let mut bytes = vec![fs::read(parent.join("root/root.json")).unwrap()];
    if let Some(id) = id {
        for path in [
            parent.join("root/registry").join(id).join("data/redo.wal"),
            parent.join("root/private").join(id).join("redo.wal"),
            parent
                .join("file-parent")
                .join(id)
                .join("metadata/redo.wal"),
        ] {
            bytes.push(fs::read(path).unwrap());
        }
        let mut paths = fs::read_dir(parent.join("file-parent").join(id).join("objects"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        paths.sort();
        bytes.extend(paths.iter().map(|p| fs::read(p).unwrap()));
    }
    bytes
}
#[test]
fn actual_backup_verify_restore_cycle_uses_explicit_scopes_and_only_redacted_count_receipts() {
    let _serial = CASES.lock().unwrap();
    for empty in [false, true] {
        for compact in [false, true] {
            let t = tempfile::tempdir().unwrap();
            let (expected, id, directory) = prepare(t.path(), empty, compact);
            let before = histories(t.path(), id.as_deref());
            let target = t.path().join("новый архив 界.bin");
            let ids = id.as_deref().into_iter().collect::<Vec<_>>();
            let output = command(
                t.path(),
                &target,
                (!empty).then_some(directory.as_path()),
                &ids,
                None,
            );
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let report: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report.as_object().unwrap().len(), 9);
            assert_eq!(report["file_stores"], usize::from(!empty));
            assert_eq!(report["physical_objects"], usize::from(!empty) * 2);
            assert_eq!(report["reset_at"], "50");
            let text = String::from_utf8(output.stdout).unwrap();
            for private in [
                "private-payload",
                "private-display",
                "private-project-display",
                "$argon2id$",
            ] {
                assert!(!text.contains(private));
            }
            if let Some(id) = &id {
                assert!(!text.contains(id));
            }
            assert_eq!(fs::read(&target).unwrap(), expected);
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o7777,
                0o600
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&run(t.path(), &target, None).stdout).unwrap(),
                report
            );
            let out = t.path().join("copies");
            fs::DirBuilder::new().mode(0o700).create(&out).unwrap();
            let copy = out.join("copy");
            let restored = Command::new(env!("CARGO_BIN_EXE_emilybase"))
                .args(["account-file-restore"])
                .arg(&target)
                .arg(&copy)
                .args(["--time", "40"])
                .output()
                .unwrap();
            assert!(
                restored.status.success(),
                "{}",
                String::from_utf8_lossy(&restored.stderr)
            );
            if let Some(id) = &id {
                let mut native =
                    NativeFileRoot::open(copy.join("files").join(id), id.parse().unwrap()).unwrap();
                assert_eq!(native.list().unwrap().len(), 1);
                let mut reader = native.reader(FileId::from_bytes([2; 16])).unwrap();
                let mut payload = [0; 15];
                assert_eq!(reader.read_payload(&mut payload).unwrap(), 15);
                assert_eq!(&payload, b"private-payload");
            }
            assert_eq!(histories(t.path(), id.as_deref()), before);
        }
    }
}
#[test]
fn explicit_subset_does_not_discover_or_create_file_roots_and_invalid_lists_precede_owner_open() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (_, id, directory) = prepare(t.path(), false, false);
    let id = id.unwrap();
    let before = histories(t.path(), Some(&id));
    let root = AccountRoot::open(t.path().join("root"), PasswordPool::new(1).unwrap()).unwrap();
    let target = t.path().join("new");
    for (parent, ids) in [
        (Some(directory.as_path()), vec![]),
        (None, vec![id.as_str()]),
        (Some(directory.as_path()), vec![id.as_str(), id.as_str()]),
        (Some(directory.as_path()), vec!["../bad"]),
        (Some(directory.as_path()), vec![id.as_str(); 5]),
    ] {
        let output = command(t.path(), &target, parent, &ids, None);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("already locked"));
        refused(output);
        assert!(!target.exists());
    }
    drop(root);
    let output = command(
        t.path(),
        &target,
        Some(&t.path().join("missing")),
        &["00000000000000000000000000000000"],
        None,
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("declared private roster"));
    refused(output);
    let output = command(t.path(), &target, None, &[], None);
    assert!(output.status.success());
    let value = emilybase_server::inspect_account_file_archive(&target).unwrap();
    assert_eq!(value.manifest().file_stores().len(), 0);
    assert_eq!(value.manifest().private_projects().len(), 1);
    assert_eq!(histories(t.path(), Some(&id)), before);
}
#[test]
fn actual_cli_refuses_output_within_account_and_file_sources_without_stages_or_source_changes() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (_, id, directory) = prepare(t.path(), false, true);
    let id = id.unwrap();
    let before = histories(t.path(), Some(&id));
    for path in [
        t.path().join("root"),
        t.path().join("root/private").join(&id),
        t.path().join("root/registry").join(&id).join("data"),
        directory.join(&id),
        directory.join(&id).join("objects"),
    ] {
        let count = fs::read_dir(&path).unwrap().count();
        refused(command(
            t.path(),
            &path.join("archive"),
            Some(&directory),
            &[&id],
            None,
        ));
        assert_eq!(fs::read_dir(path).unwrap().count(), count);
    }
    assert_eq!(histories(t.path(), Some(&id)), before);
}
#[test]
fn conflicts_busy_owners_missing_components_and_parent_aliases_never_overwrite_or_provision() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (_, id, directory) = prepare(t.path(), false, false);
    let id = id.unwrap();
    let before = histories(t.path(), Some(&id));
    let target = t.path().join("new");
    fs::write(&target, b"existing").unwrap();
    refused(command(t.path(), &target, Some(&directory), &[&id], None));
    assert_eq!(fs::read(&target).unwrap(), b"existing");
    fs::remove_file(&target).unwrap();
    let original = t.path().join("архив 界 с пробелами.bin");
    symlink(&original, &target).unwrap();
    refused(command(t.path(), &target, Some(&directory), &[&id], None));
    assert_eq!(fs::read_link(&target).unwrap(), original);
    fs::remove_file(&target).unwrap();
    let native = NativeFileRoot::open(directory.join(&id), id.parse().unwrap()).unwrap();
    refused(command(t.path(), &target, Some(&directory), &[&id], None));
    assert!(!target.exists());
    drop(native);
    let alias = t.path().join("file-alias");
    symlink(&directory, &alias).unwrap();
    refused(command(t.path(), &target, Some(&alias), &[&id], None));
    assert!(!target.exists());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    refused(command(t.path(), &target, Some(&directory), &[&id], None));
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::rename(directory.join(&id), directory.join("saved")).unwrap();
    refused(command(t.path(), &target, Some(&directory), &[&id], None));
    assert!(!directory.join(&id).exists());
    assert!(!target.exists());
    fs::rename(directory.join("saved"), directory.join(&id)).unwrap();
    assert_eq!(histories(t.path(), Some(&id)), before);
}
#[test]
fn failed_stdout_keeps_the_completed_private_archive_and_unchanged_source_histories() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (expected, id, directory) = prepare(t.path(), false, true);
    let id = id.unwrap();
    let before = histories(t.path(), Some(&id));
    let target = t.path().join("archive");
    refused(command(
        t.path(),
        &target,
        Some(&directory),
        &[&id],
        Some(File::options().write(true).open("/dev/full").unwrap()),
    ));
    assert_eq!(fs::read(&target).unwrap(), expected);
    emilybase_server::inspect_account_file_archive(&target).unwrap();
    assert_eq!(histories(t.path(), Some(&id)), before);
}
