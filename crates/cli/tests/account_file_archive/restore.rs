use super::*;
use emilybase_auth::accounts::AccountStore;
use std::os::unix::fs::DirBuilderExt;
use std::time::{Duration, Instant};

fn out(parent: &Path) -> PathBuf {
    let directory = parent.join("destinations");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    directory
}
fn restore(
    parent: &Path,
    archive: &Path,
    target: &Path,
    time: &str,
    stdout: Option<File>,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_emilybase"))
        .current_dir(parent)
        .arg("account-file-restore")
        .arg(archive)
        .arg(target)
        .arg("--time")
        .arg(time)
        .stdin(Stdio::null())
        .stdout(stdout.map(Stdio::from).unwrap_or_else(Stdio::piped))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("bounded operator command exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}
fn copied(target: &Path, project: Option<&str>) {
    let manifest = emilybase_server::inspect_account_file_root_manifest_bytes(
        &fs::read(target.join("root.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.reset_at(), 40);
    assert_eq!(
        manifest.private_projects().len(),
        usize::from(project.is_some())
    );
    assert!(AccountRoot::open(target, PasswordPool::new(1).unwrap()).is_err());
    if let Some(id) = project {
        let store = AccountStore::open(
            target.join("private").join(id),
            id,
            PasswordPool::new(1).unwrap(),
        )
        .unwrap();
        assert_eq!(store.session_clock_floor().unwrap(), Some(40));
        let mut files =
            NativeFileRoot::open(target.join("files").join(id), id.parse().unwrap()).unwrap();
        assert_eq!(files.list().unwrap().len(), 1);
        let mut reader = files.reader(FileId::from_bytes([2; 16])).unwrap();
        let mut bytes = [0; 15];
        assert_eq!(reader.read_payload(&mut bytes).unwrap(), 15);
        assert_eq!(&bytes, b"private-payload");
    }
}
#[test]
fn actual_restore_command_emits_only_counts_and_creates_independently_readable_offline_copies() {
    let _serial = CASES.lock().unwrap();
    for empty in [false, true] {
        for compact in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (archive, bytes, project) = fixture(temp.path(), empty, compact);
            if compact {
                fs::set_permissions(&archive, fs::Permissions::from_mode(0o400)).unwrap();
            }
            let directory = out(temp.path());
            let target = directory.join("копия 界 с пробелами");
            let output = restore(
                temp.path(),
                Path::new("./архив 界 с пробелами.bin"),
                &target,
                "40",
                None,
            );
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 7);
            assert_eq!(value["root_format"], 2);
            for field in ["projects", "private_projects", "file_stores", "references"] {
                assert_eq!(value[field], usize::from(!empty));
            }
            assert_eq!(value["physical_objects"], usize::from(!empty) * 2);
            assert_eq!(value["reset_at"], "40");
            let text = String::from_utf8(output.stdout).unwrap();
            for secret in [
                "private-payload",
                "private-display",
                "private-project-display",
                "$argon2id$",
                "secret",
                "token",
            ] {
                assert!(!text.contains(secret));
            }
            copied(&target, project.as_deref());
            assert_eq!(fs::read(&archive).unwrap(), bytes);
            assert_eq!(
                fs::metadata(&archive).unwrap().permissions().mode() & 0o7777,
                if compact { 0o400 } else { 0o600 }
            );
        }
    }
}
#[test]
fn actual_command_refuses_private_source_type_mode_size_and_time_before_publication() {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, _) = fixture(temp.path(), true, false);
    let directory = out(temp.path());
    let target = directory.join("copy");
    for mode in [0o644, 0o4600] {
        fs::set_permissions(&archive, fs::Permissions::from_mode(mode)).unwrap();
        refused(restore(temp.path(), &archive, &target, "40", None));
        assert!(!target.exists());
    }
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = temp.path().join("alias");
    symlink(&archive, &alias).unwrap();
    refused(restore(temp.path(), &alias, &target, "40", None));
    let link = temp.path().join("linked");
    fs::hard_link(&archive, &link).unwrap();
    refused(restore(temp.path(), &link, &target, "40", None));
    fs::remove_file(link).unwrap();
    refused(restore(temp.path(), &directory, &target, "40", None));
    let fifo = temp.path().join("fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    refused(restore(temp.path(), &fifo, &target, "40", None));
    let sparse = temp.path().join("oversized");
    File::create(&sparse)
        .unwrap()
        .set_len(emilybase_server::MAX_ACCOUNT_FILE_ARCHIVE_BYTES as u64 + 1)
        .unwrap();
    fs::set_permissions(&sparse, fs::Permissions::from_mode(0o600)).unwrap();
    refused(restore(temp.path(), &sparse, &target, "40", None));
    let bad = temp.path().join("bad");
    fs::write(&bad, b"invalid immutable input").unwrap();
    fs::set_permissions(&bad, fs::Permissions::from_mode(0o600)).unwrap();
    refused(restore(temp.path(), &bad, &target, "40", None));
    for time in ["18446744073709551615", "bad", "-1"] {
        refused(restore(temp.path(), &archive, &target, time, None));
    }
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}
#[test]
fn existing_file_directory_symlink_and_source_targets_remain_unchanged() {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, _) = fixture(temp.path(), false, false);
    let directory = out(temp.path());
    let file = directory.join("file");
    fs::write(&file, b"keep existing").unwrap();
    let folder = directory.join("directory");
    fs::DirBuilder::new().mode(0o700).create(&folder).unwrap();
    fs::write(folder.join("keep"), b"keep directory").unwrap();
    let alias = directory.join("alias");
    symlink(&archive, &alias).unwrap();
    for target in [&file, &folder, &alias, &archive] {
        refused(restore(temp.path(), &archive, target, "40", None));
    }
    assert_eq!(fs::read(file).unwrap(), b"keep existing");
    assert_eq!(fs::read(folder.join("keep")).unwrap(), b"keep directory");
    assert_eq!(fs::read_link(alias).unwrap(), archive);
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}
#[test]
fn lost_operator_output_keeps_the_completed_copy_without_leaking_contents_or_rollback() {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, project) = fixture(temp.path(), false, true);
    let directory = out(temp.path());
    let target = directory.join("copy");
    let full = File::options().write(true).open("/dev/full").unwrap();
    refused(restore(temp.path(), &archive, &target, "40", Some(full)));
    assert!(target.exists());
    copied(&target, project.as_deref());
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}
#[test]
fn missing_trusted_time_and_invalid_native_destination_refuse_without_panicking() {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, _) = fixture(temp.path(), true, false);
    let directory = out(temp.path());
    let target = directory.join("copy");
    refused(
        Command::new(env!("CARGO_BIN_EXE_emilybase"))
            .arg("account-file-restore")
            .arg(&archive)
            .arg(&target)
            .output()
            .unwrap(),
    );
    refused(restore(
        temp.path(),
        &archive,
        &archive.join("not-a-parent"),
        "40",
        None,
    ));
    use std::os::unix::ffi::OsStringExt;
    let non_utf8 = directory.join(std::ffi::OsString::from_vec(vec![b'x', 255]));
    refused(restore(temp.path(), &archive, &non_utf8, "40", None));
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}

#[test]
fn native_parent_links_refuse_and_maximum_trusted_time_is_emitted_without_rounding() {
    let _serial = CASES.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, project) = fixture(temp.path(), false, false);
    let directory = out(temp.path());
    let source_parent = temp.path().join("source-parent-link");
    symlink(temp.path(), &source_parent).unwrap();
    refused(restore(
        temp.path(),
        &source_parent.join(archive.file_name().unwrap()),
        &directory.join("copy"),
        "40",
        None,
    ));
    let destination_parent = temp.path().join("destination-parent-link");
    symlink(&directory, &destination_parent).unwrap();
    refused(restore(
        temp.path(),
        &archive,
        &destination_parent.join("copy"),
        "40",
        None,
    ));
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    let target = directory.join("maximum-time");
    let output = restore(temp.path(), &archive, &target, "9223372036854775807", None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["reset_at"], "9223372036854775807");
    let id = project.unwrap();
    let copied = AccountStore::open(
        target.join("private").join(id.clone()),
        &id,
        PasswordPool::new(1).unwrap(),
    )
    .unwrap();
    assert_eq!(copied.session_clock_floor().unwrap(), Some(i64::MAX as u64));
    assert_eq!(fs::read(&archive).unwrap(), bytes);
}

#[test]
fn actual_command_accepts_a_non_utf8_native_source_leaf_without_printing_it() {
    let _serial = CASES.lock().unwrap();
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let (archive, bytes, _) = fixture(temp.path(), true, false);
    let non_utf8 = temp
        .path()
        .join(std::ffi::OsString::from_vec(vec![b'a', 255]));
    fs::copy(&archive, &non_utf8).unwrap();
    let directory = out(temp.path());
    let target = directory.join("copy");
    let output = restore(temp.path(), &non_utf8, &target, "40", None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    copied(&target, None);
    assert_eq!(fs::read(non_utf8).unwrap(), bytes);
}
