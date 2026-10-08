use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase-server"));
    for name in [
        "EMILYBASE_MASTER_KEY",
        "EMILYBASE_MASTER_KEY_FILE",
        "EMILYBASE_DATA_DIR",
        "EMILYBASE_ACCOUNT_ROOT",
        "EMILYBASE_LISTEN",
    ] {
        command.env_remove(name);
    }
    command
}
#[test]
fn invalid_configuration_never_creates_data_or_echoes_secret() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("absent");
    for key in [None, Some("secret_that_must_not_appear")] {
        let mut command = command();
        command.env("EMILYBASE_DATA_DIR", &root);
        if let Some(key) = key {
            command.env("EMILYBASE_MASTER_KEY", key);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(!root.exists());
        let log = String::from_utf8(output.stdout).unwrap();
        assert!(!log.contains("secret_that_must_not_appear"));
        assert!(log.contains("startup_failed"));
    }
}
#[test]
fn binary_logs_only_route_patterns_and_sigterm_stops_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    let master = "a".repeat(64); // Synthetic, disposable fixture.
    let mut child = command()
        .env("EMILYBASE_MASTER_KEY", &master)
        .env("EMILYBASE_DATA_DIR", &root)
        .env("EMILYBASE_LISTEN", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut log = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.contains("experimental_server_listening") {
                let value: serde_json::Value = serde_json::from_str(&line).unwrap();
                send.send(value["fields"]["address"].as_str().unwrap().to_owned())
                    .unwrap();
            }
            log.push_str(&line);
            log.push('\n');
        }
        log
    });
    let address = match receive.recv_timeout(Duration::from_secs(10)) {
        Ok(address) => address,
        Err(error) => {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("startup: {error}");
        }
    };
    let mut socket = TcpStream::connect(&address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let id = "e".repeat(32);
    write!(socket,"POST /v1/projects/{id}/sql?private_query=hidden HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {master}\r\nContent-Type: application/json\r\nContent-Length: 6\r\nConnection: close\r\n\r\nhidden").unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 401"));
    // HTTP extension methods are peer-controlled text, including valid token-shaped secrets.
    for (method, key, status) in [
        ("SYNTHETIC_PRIVATE_METHOD", master.as_str(), 405),
        (master.as_str(), master.as_str(), 405),
        ("SYNTHETIC_DENIED_METHOD", "invalid", 401),
    ] {
        let mut socket = TcpStream::connect(&address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(socket,"{method} /v1/projects HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("shutdown timeout");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let log = reader.join().unwrap();
    assert!(log.contains("/v1/projects/{id}/sql"));
    assert!(log.contains("graceful_shutdown_requested"));
    for private in [
        master.as_str(),
        id.as_str(),
        "private_query",
        "hidden",
        "SYNTHETIC_PRIVATE_METHOD",
        "SYNTHETIC_DENIED_METHOD",
    ] {
        assert!(!log.contains(private));
    }
    let methods: Vec<_> = log
        .lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["fields"]["method"].as_str().map(str::to_owned)
        })
        .collect();
    assert_eq!(methods, ["POST", "OTHER", "OTHER", "OTHER"]);
    let mut errors = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut errors)
        .unwrap();
    assert!(errors.is_empty());
    drop(emilybase_server::ProjectStore::open(root).unwrap());
}
