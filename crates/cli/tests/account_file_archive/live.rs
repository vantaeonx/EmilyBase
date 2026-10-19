use super::*;
use std::os::unix::fs::DirBuilderExt;

fn prepared(
    parent: &Path,
    empty: bool,
    compact: bool,
    time: u64,
) -> (PathBuf, Option<String>, Vec<u8>) {
    let (archive, bytes, id) = fixture(parent, empty, compact);
    let out = parent.join("copies");
    fs::DirBuilder::new().mode(0o700).create(&out).unwrap();
    let path = out.join("корень 界 v2");
    emilybase_server::restore_account_file_archive(
        &archive,
        &path,
        PasswordPool::new(1).unwrap(),
        time,
    )
    .unwrap();
    (path, id, bytes)
}
fn info(parent: &Path, path: &Path, stdout: Option<File>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .current_dir(parent)
        .arg("account-file-root-info")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(stdout.map(Stdio::from).unwrap_or_else(Stdio::piped))
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}
fn histories(path: &Path, id: Option<&str>) -> Vec<Vec<u8>> {
    let mut result = vec![fs::read(path.join("root.json")).unwrap()];
    if let Some(id) = id {
        for item in [
            path.join("registry").join(id).join("data/redo.wal"),
            path.join("private").join(id).join("redo.wal"),
            path.join("files").join(id).join("metadata/redo.wal"),
        ] {
            result.push(fs::read(item).unwrap());
        }
        let mut objects = fs::read_dir(path.join("files").join(id).join("objects"))
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect::<Vec<_>>();
        objects.sort();
        result.extend(objects.into_iter().map(|p| fs::read(p).unwrap()));
    }
    result
}
#[test]
fn actual_v2_info_opens_healthy_copies_emits_five_private_counts_and_preserves_histories() {
    let _serial = CASES.lock().unwrap();
    for empty in [false, true] {
        for compact in [false, true] {
            let t = tempfile::tempdir().unwrap();
            let (path, id, _) = prepared(t.path(), empty, compact, 40);
            let before = histories(&path, id.as_deref());
            let output = info(t.path(), &path, None);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 5);
            assert_eq!(value["root_format"], 2);
            for field in ["projects", "private_projects", "file_stores"] {
                assert_eq!(value[field], usize::from(!empty));
            }
            assert_eq!(value["reset_at"], "40");
            let text = String::from_utf8(output.stdout).unwrap();
            for secret in [
                "private-display",
                "private-project-display",
                "private-payload",
                "$argon2id$",
            ] {
                assert!(!text.contains(secret));
            }
            if let Some(id) = &id {
                assert!(!text.contains(id));
            }
            assert_eq!(histories(&path, id.as_deref()), before);
        }
    }
}
#[test]
fn v2_info_refuses_v1_and_legacy_commands_refuse_v2_without_dropping_bindings_or_creating_outputs()
{
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (path, id, _) = prepared(t.path(), false, false, 40);
    let before = histories(&path, id.as_deref());
    refused(info(t.path(), &t.path().join("root"), None));
    for command in ["account-root-verify", "account-root-projects"] {
        refused(
            Command::new(env!("CARGO_BIN_EXE_emilybase"))
                .arg(command)
                .arg(&path)
                .output()
                .unwrap(),
        );
    }
    let target = t.path().join("must-not-exist");
    refused(
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("account-file-backup")
            .arg(&path)
            .arg(&target)
            .output()
            .unwrap(),
    );
    assert!(!target.exists());
    assert_eq!(histories(&path, id.as_deref()), before);
}
#[test]
fn v2_info_refuses_busy_owners_wrong_bindings_links_modes_and_corruption_without_repairs() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (path, id, _) = prepared(t.path(), false, true, 40);
    let id = id.unwrap();
    let before = histories(&path, Some(&id));
    let native = NativeFileRoot::open(path.join("files").join(&id), id.parse().unwrap()).unwrap();
    refused(info(t.path(), &path, None));
    drop(native);
    assert_eq!(histories(&path, Some(&id)), before);
    let alias = t.path().join("alias");
    symlink(&path, &alias).unwrap();
    refused(info(t.path(), &alias, None));
    fs::set_permissions(path.join("files"), fs::Permissions::from_mode(0o755)).unwrap();
    refused(info(t.path(), &path, None));
    fs::set_permissions(path.join("files"), fs::Permissions::from_mode(0o700)).unwrap();
    let m = emilybase_server::inspect_account_file_root_manifest_bytes(&before[0]).unwrap();
    let wrong =
        emilybase_server::FileRootBinding::new(&id, "00000000000000000000000000000000").unwrap();
    let invalid = emilybase_server::AccountFileRootManifest::new(
        m.private_projects().to_vec(),
        vec![wrong],
        40,
    )
    .unwrap()
    .to_bytes()
    .unwrap();
    fs::write(path.join("root.json"), &invalid).unwrap();
    refused(info(t.path(), &path, None));
    assert_eq!(fs::read(path.join("root.json")).unwrap(), invalid);
    fs::write(path.join("root.json"), &before[0]).unwrap();
    let wal = path.join("files").join(&id).join("metadata/redo.wal");
    let mut corrupt = fs::read(&wal).unwrap();
    corrupt[0] ^= 1;
    fs::write(&wal, &corrupt).unwrap();
    refused(info(t.path(), &path, None));
    assert_eq!(fs::read(wal).unwrap(), corrupt);
}
#[test]
fn failed_v2_info_output_releases_all_owners_and_preserves_complete_existing_sources() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (path, id, bytes) = prepared(t.path(), false, true, 40);
    let before = histories(&path, id.as_deref());
    refused(info(
        t.path(),
        &path,
        Some(File::options().write(true).open("/dev/full").unwrap()),
    ));
    assert_eq!(histories(&path, id.as_deref()), before);
    let root = AccountRoot::open_v2(&path, PasswordPool::new(1).unwrap()).unwrap();
    assert!(root.file_manifest().unwrap().is_some());
    drop(root);
    assert_eq!(
        fs::read(t.path().join("архив 界 с пробелами.bin")).unwrap(),
        bytes
    );
}
#[test]
fn maximum_exact_time_and_native_non_utf8_root_names_are_admitted_without_name_output() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (path, id, _) = prepared(t.path(), true, false, i64::MAX as u64);
    let native = path
        .parent()
        .unwrap()
        .join(OsString::from_vec(vec![b'v', 0xff]));
    fs::rename(&path, &native).unwrap();
    let before = histories(&native, id.as_deref());
    let output = info(t.path(), &native, None);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["reset_at"], i64::MAX.to_string());
    assert_eq!(histories(&native, id.as_deref()), before);
}

fn backup_v2(path: &Path, target: &Path, stdout: Option<File>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .arg("account-file-backup-v2")
        .arg(path)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(stdout.map(Stdio::from).unwrap_or_else(Stdio::piped))
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}
#[test]
fn actual_v2_backup_verify_restore_roundtrip_preserves_every_binding_and_prints_only_counts() {
    let _serial = CASES.lock().unwrap();
    for empty in [false, true] {
        for compact in [false, true] {
            let t = tempfile::tempdir().unwrap();
            let (path, id, _) = prepared(t.path(), empty, compact, 40);
            let before = histories(&path, id.as_deref());
            let archive = t.path().join("повторная копия 界");
            let output = backup_v2(&path, &archive, None);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 9);
            assert_eq!(value["file_stores"], usize::from(!empty));
            assert_eq!(value["reset_at"], "40");
            let text = String::from_utf8(output.stdout).unwrap();
            for secret in [
                "private-display",
                "private-payload",
                "private-project-display",
                "$argon2id$",
            ] {
                assert!(!text.contains(secret));
            }
            if let Some(id) = &id {
                assert!(!text.contains(id));
            }
            let checked = run(t.path(), &archive, None);
            assert!(checked.status.success());
            assert_eq!(
                serde_json::from_slice::<Value>(&checked.stdout).unwrap(),
                value
            );
            let report = emilybase_server::inspect_account_file_archive(&archive).unwrap();
            assert_eq!(report.manifest().to_bytes().unwrap(), before[0]);
            let target = t.path().join("second");
            let restored = Command::new(env!("CARGO_BIN_EXE_emilybase"))
                .arg("account-file-restore")
                .arg(&archive)
                .arg(&target)
                .args(["--time", "60"])
                .output()
                .unwrap();
            assert!(restored.status.success());
            assert!(info(t.path(), &target, None).status.success());
            assert_eq!(histories(&path, id.as_deref()), before);
        }
    }
}
#[test]
fn actual_v2_backup_refuses_legacy_busy_contained_and_existing_outputs_without_source_changes() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (path, id, _) = prepared(t.path(), false, true, 40);
    let before = histories(&path, id.as_deref());
    let absent = t.path().join("absent");
    refused(backup_v2(&t.path().join("root"), &absent, None));
    assert!(!absent.exists());
    let root = AccountRoot::open_v2(&path, PasswordPool::new(1).unwrap()).unwrap();
    refused(backup_v2(&path, &absent, None));
    drop(root);
    let contained = path
        .join("files")
        .join(id.as_ref().unwrap())
        .join("objects/archive");
    refused(backup_v2(&path, &contained, None));
    assert!(!contained.exists());
    fs::write(&absent, b"existing").unwrap();
    fs::set_permissions(&absent, fs::Permissions::from_mode(0o600)).unwrap();
    refused(backup_v2(&path, &absent, None));
    assert_eq!(fs::read(absent).unwrap(), b"existing");
    assert_eq!(histories(&path, id.as_deref()), before);
}
#[test]
fn failed_actual_v2_backup_stdout_preserves_the_complete_verifiable_archive_and_releases_sources() {
    let _serial = CASES.lock().unwrap();
    let t = tempfile::tempdir().unwrap();
    let (path, id, _) = prepared(t.path(), false, false, 40);
    let before = histories(&path, id.as_deref());
    let archive = t.path().join("archive");
    refused(backup_v2(
        &path,
        &archive,
        Some(File::options().write(true).open("/dev/full").unwrap()),
    ));
    let report = emilybase_server::inspect_account_file_archive(&archive).unwrap();
    assert_eq!(report.manifest().to_bytes().unwrap(), before[0]);
    assert_eq!(histories(&path, id.as_deref()), before);
    assert!(AccountRoot::open_v2(&path, PasswordPool::new(1).unwrap()).is_ok());
}
