use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Server {
    child: Child,
    reader: Option<JoinHandle<String>>,
    pub address: SocketAddr,
}
impl Server {
    pub fn start(root: &Path, master: &str) -> Self {
        Self::start_mode(root, master, false)
    }
    #[allow(
        dead_code,
        reason = "Shared fixture also compiles in the independent legacy network test binary"
    )]
    pub fn start_account(root: &Path, master: &str) -> Self {
        Self::start_mode(root, master, true)
    }
    fn start_mode(root: &Path, master: &str, private: bool) -> Self {
        Self::start_source(root, Some(master), None, private)
    }
    #[allow(
        dead_code,
        reason = "Shared fixture also compiles in network binaries without file-key cases"
    )]
    pub fn start_file(root: &Path, path: &Path) -> Self {
        Self::start_source(root, None, Some(path), false)
    }
    fn start_source(root: &Path, master: Option<&str>, path: Option<&Path>, private: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_emilybase-server"));
        command
            .env_remove("EMILYBASE_MASTER_KEY")
            .env_remove("EMILYBASE_MASTER_KEY_FILE")
            .env_remove("EMILYBASE_DATA_DIR")
            .env_remove("EMILYBASE_ACCOUNT_ROOT")
            .env(
                if private {
                    "EMILYBASE_ACCOUNT_ROOT"
                } else {
                    "EMILYBASE_DATA_DIR"
                },
                root,
            )
            .env("EMILYBASE_LISTEN", "127.0.0.1:0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(master) = master {
            command.env("EMILYBASE_MASTER_KEY", master);
        }
        if let Some(path) = path {
            command.env("EMILYBASE_MASTER_KEY_FILE", path);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut log = String::new();
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                let value: Value = serde_json::from_str(&line).unwrap();
                if value["fields"]["message"] == "experimental_server_listening" {
                    let address = value["fields"]["address"]
                        .as_str()
                        .unwrap()
                        .parse::<SocketAddr>()
                        .unwrap();
                    let _ = sender.send(address);
                }
                log.push_str(&line);
                log.push('\n');
            }
            log
        });
        let mut server = Self {
            child,
            reader: Some(reader),
            address: "127.0.0.1:0".parse().unwrap(),
        };
        server.address = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("server startup deadline");
        server
    }
    pub fn signal(&self) {
        assert!(
            Command::new("kill")
                .args(["-TERM", &self.child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
    }
    pub fn kill(mut self) -> String {
        self.child.kill().unwrap();
        self.finish(false)
    }
    pub fn stop(mut self) -> String {
        self.signal();
        self.finish(true)
    }
    pub fn drain(mut self) -> String {
        self.finish(true)
    }
    fn finish(&mut self, success: bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.success(), success);
                break;
            }
            assert!(Instant::now() < deadline, "server shutdown deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut errors = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut errors)
            .unwrap();
        assert!(errors.is_empty());
        self.reader.take().unwrap().join().unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
pub fn socket(address: SocketAddr) -> std::io::Result<TcpStream> {
    let socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(socket)
}
pub fn headers(method: &str, path: &str, key: &str, size: usize) -> String {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
    )
}
pub fn read(mut socket: TcpStream) -> std::io::Result<(u16, Value)> {
    let mut response = String::new();
    socket.read_to_string(&mut response)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("incomplete HTTP response"))?;
    let status = headers
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| std::io::Error::other("invalid HTTP status"))?;
    let json =
        serde_json::from_str(body).map_err(|_| std::io::Error::other("invalid HTTP JSON"))?;
    Ok((status, json))
}
pub fn call(
    address: SocketAddr,
    method: &str,
    path: &str,
    key: &str,
    payload: &Value,
) -> std::io::Result<(u16, Value)> {
    let body = serde_json::to_string(payload)?;
    let mut socket = socket(address)?;
    socket.write_all(headers(method, path, key, body.len()).as_bytes())?;
    socket.write_all(body.as_bytes())?;
    read(socket)
}
