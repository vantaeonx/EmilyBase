// The same actual process fixture serves independent network test binaries.
#[allow(dead_code)]
mod support;
use emilybase_auth::{accounts::AccountStore, password::PasswordPool};
use emilybase_files::{FileId, FileQuota, NativeFileRoot, initialize_file_root};
use emilybase_object_storage::{ObjectId, ProjectDirectory};
use emilybase_server::{AccountRoot, ProjectStore};
use emilybase_transactions::Database;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

static CASES: std::sync::Mutex<()> = std::sync::Mutex::new(());
const MASTER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PASSWORD: &[u8] = b"synthetic-private-password";
type History = BTreeMap<PathBuf, (u32, Vec<u8>)>;
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    legacy: PathBuf,
    native: PathBuf,
    credentials: Vec<(String, String)>,
    original: (History, History),
}
fn pool() -> PasswordPool {
    PasswordPool::new(1).unwrap()
}
fn history(root: &Path) -> History {
    fn walk(root: &Path, path: &Path, out: &mut History) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let m = entry.metadata().unwrap();
            if m.is_dir() {
                walk(root, &entry.path(), out);
            } else {
                assert!(m.is_file());
                out.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    (m.mode(), fs::read(entry.path()).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
fn fixture(count: usize, compact: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut registry = ProjectStore::open(dir.path().join("registry")).unwrap();
    let private = dir.path().join("private");
    let native = dir.path().join("native");
    for path in [&private, &native] {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    let mut accounts = Vec::new();
    let mut credentials = Vec::new();
    for _ in 0..count {
        let project = registry.create("synthetic-private-project").unwrap();
        let id = project.project.id;
        registry
            .authorize(&id, &project.api_key)
            .unwrap()
            .execute(
                "CREATE TABLE t(id INT PRIMARY KEY,v INT);INSERT INTO t VALUES(1,0)",
                &[],
            )
            .unwrap();
        let mut account = AccountStore::create(private.join(&id), &id, pool()).unwrap();
        account.enable_session_clock(10).unwrap();
        account.create_user("synthetic_user", PASSWORD).unwrap();
        let path = native.join(&id);
        let scope = id.parse().unwrap();
        initialize_file_root(&path, scope, FileQuota::new(8, 1024).unwrap()).unwrap();
        let mut file = NativeFileRoot::open(&path, scope).unwrap();
        file.publish(
            FileId::from_bytes([2; 16]),
            ObjectId::from_bytes([3; 16]),
            [4; 16],
            "synthetic-file",
            b"synthetic-payload",
        )
        .unwrap();
        drop(file);
        ProjectDirectory::open(path.join("objects"), scope)
            .unwrap()
            .put(ObjectId::from_bytes([5; 16]), b"synthetic-orphan")
            .unwrap();
        if compact {
            account.compact().unwrap();
            for path in [
                dir.path().join("registry").join(&id).join("data"),
                path.join("metadata"),
            ] {
                Database::open(path).unwrap().compact().unwrap();
            }
        }
        accounts.push(account);
        credentials.push((id, project.api_key));
    }
    let image = registry.capture_account_bundle(&mut accounts).unwrap();
    drop(accounts);
    drop(registry);
    credentials.sort();
    let legacy = dir.path().join("legacy");
    emilybase_server::restore_account_bundle_bytes(&image, &legacy, pool(), 40).unwrap();
    let mut root = AccountRoot::open(&legacy, pool()).unwrap();
    let mut files = credentials
        .iter()
        .map(|(id, _)| NativeFileRoot::open(native.join(id), id.parse().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let snapshot =
        emilybase_server::capture_account_file_components(&mut root, &mut files).unwrap();
    let archive = dir.path().join("initial.archive");
    emilybase_server::publish_account_file_archive(&snapshot, &archive).unwrap();
    drop(files);
    drop(root);
    let root = dir.path().join("root-v2");
    emilybase_server::restore_account_file_archive(&archive, &root, pool(), 40).unwrap();
    let original = (history(&legacy), history(&native));
    Fixture {
        _dir: dir,
        root,
        legacy,
        native,
        credentials,
        original,
    }
}
impl Fixture {
    fn original_unchanged(&self) {
        assert_eq!(history(&self.legacy), self.original.0);
        assert_eq!(history(&self.native), self.original.1);
    }
    fn owner_checks(&self) {
        assert!(File::open(&self.root).unwrap().try_lock().is_err());
        for (id, _) in &self.credentials {
            assert!(AccountStore::open(self.root.join("private").join(id), id, pool()).is_err());
            assert!(
                NativeFileRoot::open(self.root.join("files").join(id), id.parse().unwrap())
                    .is_err()
            );
        }
    }
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase-server"));
    for name in [
        "EMILYBASE_MASTER_KEY",
        "EMILYBASE_MASTER_KEY_FILE",
        "EMILYBASE_DATA_DIR",
        "EMILYBASE_ACCOUNT_ROOT",
        "EMILYBASE_ACCOUNT_LAYOUT",
        "EMILYBASE_LISTEN",
    ] {
        command.env_remove(name);
    }
    command
        .env("EMILYBASE_MASTER_KEY", MASTER)
        .env("EMILYBASE_LISTEN", "127.0.0.1:0");
    command
}
fn refused(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("invalid startup selected a running server");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let log = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(log.contains("startup_failed"));
    assert!(!log.contains("experimental_server_listening"));
    assert!(output.stderr.is_empty());
    for secret in [
        MASTER,
        std::str::from_utf8(PASSWORD).unwrap(),
        "synthetic_user",
        "synthetic-payload",
        "$argon2id$",
    ] {
        assert!(!log.contains(secret));
    }
    output
}
fn auth(
    server: &support::Server,
    id: &str,
    key: &str,
    operation: &str,
    body: Value,
) -> (u16, Value) {
    support::call(
        server.address,
        "POST",
        &format!("/v1/projects/{id}/auth/{operation}"),
        key,
        &body,
    )
    .unwrap()
}
#[test]
fn actual_v2_binary_starts_empty_one_four_project_roots_without_reset_and_releases_every_owner() {
    let _serial = CASES.lock().unwrap();
    for (count, compact) in [(0, false), (1, false), (4, true)] {
        let f = fixture(count, compact);
        let before = history(&f.root);
        let server = support::Server::start_account_v2(&f.root, MASTER);
        f.owner_checks();
        assert_eq!(
            support::call(server.address, "GET", "/health", MASTER, &json!({}))
                .unwrap()
                .0,
            200
        );
        let log = server.stop();
        assert!(log.contains("graceful_shutdown_requested"));
        assert_eq!(history(&f.root), before);
        drop(AccountRoot::open_v2(&f.root, pool()).unwrap());
        f.original_unchanged();
    }
}
#[test]
fn actual_v2_http_ack_kill_reopen_preserves_scopes_sessions_and_independent_backup_restore() {
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(2, compact);
        let (id, key) = &f.credentials[0];
        let (_, sibling) = &f.credentials[1];
        let manifest = fs::read(f.root.join("root.json")).unwrap();
        let server = support::Server::start_account_v2(&f.root, MASTER);
        let route = format!("/v1/projects/{id}/sql");
        for unauthorized in [MASTER, sibling] {
            assert_eq!(
                support::call(
                    server.address,
                    "POST",
                    &route,
                    unauthorized,
                    &json!({"sql":"SELECT * FROM t"})
                )
                .unwrap()
                .0,
                401
            );
        }
        let (status, pair) = auth(
            &server,
            id,
            key,
            "sign-in",
            json!({"login":"synthetic_user","password":std::str::from_utf8(PASSWORD).unwrap()}),
        );
        assert_eq!(status, 200);
        let access = pair["access_token"].as_str().unwrap();
        let (status, ack) = support::call(
            server.address,
            "POST",
            &route,
            key,
            &json!({"sql":"UPDATE t SET v=91 WHERE id=1"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        let mut log = server.kill();
        let server = support::Server::start_account_v2(&f.root, MASTER);
        f.owner_checks();
        assert_eq!(
            auth(&server, id, key, "me", json!({"access_token":access})).0,
            200
        );
        assert_eq!(
            auth(&server, id, sibling, "me", json!({"access_token":access})).0,
            401
        );
        let (status, rows) = support::call(
            server.address,
            "POST",
            &route,
            key,
            &json!({"sql":"SELECT v FROM t WHERE id=1"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(rows["transaction"], ack["transaction"]);
        assert_eq!(rows["results"][0]["rows"][0][0]["value"], 91);
        let (status, rollback) = support::call(
            server.address,
            "POST",
            &route,
            key,
            &json!({"sql":"BEGIN;UPDATE t SET v=999 WHERE id=1;ROLLBACK"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(rollback["committed"], false);
        log += &server.kill();
        let server = support::Server::start_account_v2(&f.root, MASTER);
        assert_eq!(
            auth(&server, id, key, "me", json!({"access_token":access})).0,
            200
        );
        log += &server.stop();
        for secret in [
            MASTER,
            key,
            sibling,
            id,
            access,
            std::str::from_utf8(PASSWORD).unwrap(),
            "UPDATE t SET",
        ] {
            assert!(!log.contains(secret));
        }
        assert_eq!(fs::read(f.root.join("root.json")).unwrap(), manifest);
        let mut root = AccountRoot::open_v2(&f.root, pool()).unwrap();
        let before = history(&f.root);
        let archive = f._dir.path().join("after-restart.archive");
        emilybase_server::backup_account_file_root_v2(&mut root, &archive).unwrap();
        drop(root);
        assert_eq!(history(&f.root), before);
        let copy = f._dir.path().join("second");
        emilybase_server::restore_account_file_archive(&archive, &copy, pool(), 40).unwrap();
        let server = support::Server::start_account_v2(&copy, MASTER);
        assert_eq!(
            auth(&server, id, key, "me", json!({"access_token":access})).0,
            401
        );
        server.stop();
        let copied = AccountRoot::open_v2(&copy, pool()).unwrap();
        copied
            .execute(id, key, "UPDATE t SET v=333 WHERE id=1", &[])
            .unwrap();
        drop(copied);
        let original = AccountRoot::open_v2(&f.root, pool()).unwrap();
        assert_eq!(
            original
                .execute(id, key, "SELECT v FROM t WHERE id=1", &[])
                .unwrap()
                .results[0]
                .rows,
            vec![vec![emilybase_catalog::Value::Integer(91)]]
        );
        drop(original);
        for (id, _) in &f.credentials {
            assert_eq!(
                history(&copy.join("files").join(id)),
                history(&f.root.join("files").join(id))
            );
        }
        f.original_unchanged();
    }
}

#[test]
fn actual_binary_layout_errors_refuse_without_fallback_mutation_provisioning_or_value_echo() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let _serial = CASES.lock().unwrap();
    let f = fixture(1, true);
    let before = history(&f.root);
    let absent = f._dir.path().join("must-not-exist");
    for layout in [
        None,
        Some(OsString::from("v1")),
        Some(OsString::new()),
        Some("V2".into()),
        Some(" v2".into()),
        Some("v2 ".into()),
        Some("private-invalid-layout".into()),
        Some(OsString::from_vec(vec![0xff])),
    ] {
        let mut c = command();
        c.env("EMILYBASE_ACCOUNT_ROOT", &f.root);
        if let Some(value) = layout {
            c.env("EMILYBASE_ACCOUNT_LAYOUT", value);
        }
        let output = refused(c);
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("private-invalid-layout")
        );
        assert_eq!(history(&f.root), before);
    }
    for layout in ["v1", "v2"] {
        for registry in [false, true] {
            let mut c = command();
            c.current_dir(f._dir.path())
                .env("EMILYBASE_ACCOUNT_LAYOUT", layout);
            if registry {
                c.env("EMILYBASE_DATA_DIR", &absent);
            }
            refused(c);
            assert!(!absent.exists());
            assert!(!f._dir.path().join("emilybase-data").exists());
        }
    }
    let mut c = command();
    c.env("EMILYBASE_ACCOUNT_ROOT", &f.root)
        .env("EMILYBASE_ACCOUNT_LAYOUT", "v2")
        .env("EMILYBASE_DATA_DIR", &absent);
    refused(c);
    let mut c = command();
    c.env("EMILYBASE_ACCOUNT_ROOT", &absent)
        .env("EMILYBASE_ACCOUNT_LAYOUT", "v2");
    refused(c);
    assert!(!absent.exists());
    let mut c = command();
    c.env("EMILYBASE_ACCOUNT_ROOT", &f.legacy)
        .env("EMILYBASE_ACCOUNT_LAYOUT", "v2");
    refused(c);
    assert_eq!(history(&f.root), before);
    f.original_unchanged();
    drop(AccountRoot::open_v2(&f.root, pool()).unwrap());
}
#[test]
fn explicit_binary_refuses_busy_file_owners_wrong_bindings_and_corruption_without_source_repairs() {
    let _serial = CASES.lock().unwrap();
    for kind in 0..3 {
        let f = fixture(1, false);
        let id = &f.credentials[0].0;
        let native = f.root.join("files").join(id);
        let mut owner = None;
        match kind {
            0 => {
                owner = Some(NativeFileRoot::open(&native, id.parse().unwrap()).unwrap());
            }
            1 => {
                let manifest = emilybase_server::inspect_account_file_root_manifest_bytes(
                    &fs::read(f.root.join("root.json")).unwrap(),
                )
                .unwrap();
                let wrong =
                    emilybase_server::FileRootBinding::new(id, "00000000000000000000000000000000")
                        .unwrap();
                fs::write(
                    f.root.join("root.json"),
                    emilybase_server::AccountFileRootManifest::new(
                        manifest.private_projects().to_vec(),
                        vec![wrong],
                        manifest.reset_at(),
                    )
                    .unwrap()
                    .to_bytes()
                    .unwrap(),
                )
                .unwrap();
            }
            _ => {
                let wal = native.join("metadata/redo.wal");
                let mut bytes = fs::read(&wal).unwrap();
                bytes[0] ^= 1;
                fs::write(wal, bytes).unwrap();
            }
        }
        let altered = history(&f.root);
        let mut c = command();
        c.env("EMILYBASE_ACCOUNT_ROOT", &f.root)
            .env("EMILYBASE_ACCOUNT_LAYOUT", "v2");
        refused(c);
        assert_eq!(history(&f.root), altered);
        drop(owner);
        if kind == 0 {
            drop(AccountRoot::open_v2(&f.root, pool()).unwrap());
        }
        f.original_unchanged();
    }
}
#[test]
fn native_non_utf8_v2_paths_and_master_secret_files_start_without_path_or_secret_logging() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let _serial = CASES.lock().unwrap();
    let f = fixture(0, false);
    let native = f._dir.path().join(OsString::from_vec(vec![b'r', 0xff]));
    fs::rename(&f.root, &native).unwrap();
    let before = history(&native);
    let server = support::Server::start_account_v2(&native, MASTER);
    assert_eq!(
        support::call(server.address, "GET", "/health", MASTER, &json!({}))
            .unwrap()
            .0,
        200
    );
    let log = server.stop();
    assert!(!log.contains(MASTER));
    assert_eq!(history(&native), before);
    let secret = f._dir.path().join("private-master.key");
    fs::write(&secret, MASTER).unwrap();
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    // A private file source also selects v2 without any key in the environment.
    let server = support::Server::start_account_file_v2(&native, &secret);
    assert_eq!(
        support::call(server.address, "GET", "/health", MASTER, &json!({}))
            .unwrap()
            .0,
        200
    );
    let log = server.stop();
    assert!(!log.contains(MASTER));
    assert!(!log.contains("private-master.key"));
    assert_eq!(history(&native), before);
    f.original_unchanged();
}
#[test]
fn competing_v2_process_and_busy_listen_port_never_release_or_modify_another_roots_owners() {
    let _serial = CASES.lock().unwrap();
    let f = fixture(1, true);
    let before = history(&f.root);
    let first = support::Server::start_account_v2(&f.root, MASTER);
    let mut c = command();
    c.env("EMILYBASE_ACCOUNT_ROOT", &f.root)
        .env("EMILYBASE_ACCOUNT_LAYOUT", "v2");
    refused(c);
    f.owner_checks();
    assert_eq!(
        support::call(first.address, "GET", "/health", MASTER, &json!({}))
            .unwrap()
            .0,
        200
    );
    first.stop();
    assert_eq!(history(&f.root), before);
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut c = command();
    c.env("EMILYBASE_ACCOUNT_ROOT", &f.root)
        .env("EMILYBASE_ACCOUNT_LAYOUT", "v2")
        .env("EMILYBASE_LISTEN", held.local_addr().unwrap().to_string());
    refused(c);
    assert_eq!(history(&f.root), before);
    drop(AccountRoot::open_v2(&f.root, pool()).unwrap());
    f.original_unchanged();
}
#[test]
fn partial_unacknowledged_http_body_never_becomes_a_row_after_actual_v2_process_kill() {
    use std::io::Write;
    let _serial = CASES.lock().unwrap();
    for compact in [false, true] {
        let f = fixture(1, compact);
        let (id, key) = &f.credentials[0];
        let before = history(&f.root);
        let route = format!("/v1/projects/{id}/sql");
        let server = support::Server::start_account_v2(&f.root, MASTER);
        let mut socket = support::socket(server.address).unwrap();
        let body = serde_json::to_vec(&json!({"sql":"INSERT INTO t VALUES(2,700)"})).unwrap();
        socket
            .write_all(support::headers("POST", &route, key, body.len()).as_bytes())
            .unwrap();
        socket.write_all(&body[..body.len() - 8]).unwrap();
        assert_eq!(
            support::call(server.address, "GET", "/health", MASTER, &json!({}))
                .unwrap()
                .0,
            200
        );
        let log = server.kill();
        drop(socket);
        assert!(!log.contains("INSERT INTO"));
        assert_eq!(history(&f.root), before);
        let server = support::Server::start_account_v2(&f.root, MASTER);
        let (status, rows) = support::call(
            server.address,
            "POST",
            &route,
            key,
            &json!({"sql":"SELECT id,v FROM t ORDER BY id"}),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            rows["results"][0]["rows"],
            json!([[{"type":"integer","value":1},{"type":"integer","value":0}]])
        );
        server.stop();
        assert_eq!(history(&f.root), before);
        f.original_unchanged();
    }
}
#[test]
fn explicit_v1_binary_keeps_the_legacy_layout_contract_and_never_adopts_adjacent_v2_files() {
    let _serial = CASES.lock().unwrap();
    let f = fixture(1, true);
    let v1 = history(&f.legacy);
    let v2 = history(&f.root);
    let server = support::Server::start_account_v1(&f.legacy, MASTER);
    assert_eq!(
        support::call(server.address, "GET", "/health", MASTER, &json!({}))
            .unwrap()
            .0,
        200
    );
    server.stop();
    assert_eq!(history(&f.legacy), v1);
    assert_eq!(history(&f.root), v2);
    f.original_unchanged();
}
