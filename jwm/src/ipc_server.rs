use log::{debug, info, warn};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use crate::ipc::{IpcEvent, IpcMessage, IpcResponse};

// ---------------------------------------------------------------------------
// Client wrapper
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct IpcClient {
    stream: UnixStream,
    buf: Vec<u8>,
    subscriptions: Vec<String>,
}

impl IpcClient {
    fn new(stream: UnixStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            buf: Vec::with_capacity(4096),
            subscriptions: Vec::new(),
        })
    }

    /// Try to read available data. Returns complete newline-delimited messages.
    fn read_messages(&mut self) -> io::Result<Vec<String>> {
        let mut tmp = [0u8; 4096];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "client disconnected")),
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }

        let mut messages = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]).to_string();
            if !line.is_empty() {
                messages.push(line);
            }
        }
        Ok(messages)
    }

    fn send_response(&mut self, resp: &IpcResponse) -> io::Result<()> {
        let mut json = serde_json::to_string(resp).unwrap_or_default();
        json.push('\n');
        self.stream.write_all(json.as_bytes())
    }

    fn send_event(&mut self, event: &IpcEvent) -> io::Result<()> {
        let mut json = serde_json::to_string(event).unwrap_or_default();
        json.push('\n');
        self.stream.write_all(json.as_bytes())
    }

    fn is_subscribed(&self, event_type: &str) -> bool {
        self.subscriptions.iter().any(|s| {
            s == "*" || s == event_type || event_type.starts_with(&format!("{s}/"))
        })
    }
}

// ---------------------------------------------------------------------------
// IPC Server
// ---------------------------------------------------------------------------

pub struct IpcServer {
    listener: UnixListener,
    socket_path: PathBuf,
    clients: HashMap<u64, IpcClient>,
    next_id: u64,
}

/// Parsed & validated message from a client, ready to process.
pub enum IncomingIpc {
    Command {
        client_id: u64,
        name: String,
        args: serde_json::Value,
    },
    Query {
        client_id: u64,
        name: String,
        args: serde_json::Value,
    },
    Subscribe {
        client_id: u64,
        topics: Vec<String>,
    },
}

impl IpcServer {
    /// Create and bind the IPC socket.
    pub fn new() -> io::Result<Self> {
        let path = Self::socket_path();
        // Remove stale socket if present.
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        info!("[ipc] listening on {}", path.display());
        Ok(Self {
            listener,
            socket_path: path,
            clients: HashMap::new(),
            next_id: 1,
        })
    }

    pub fn socket_path() -> PathBuf {
        let runtime = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/tmp/jwm-{}", unsafe { libc::getuid() }));
        Path::new(&runtime).join("jwm-ipc.sock")
    }

    /// Accept any pending connections.
    pub fn accept_connections(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    let id = self.next_id;
                    self.next_id += 1;
                    match IpcClient::new(stream) {
                        Ok(client) => {
                            debug!("[ipc] client {} connected", id);
                            self.clients.insert(id, client);
                        }
                        Err(e) => warn!("[ipc] failed to setup client: {e}"),
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    warn!("[ipc] accept error: {e}");
                    break;
                }
            }
        }
    }

    /// Read from all clients and return parsed messages.
    pub fn poll_clients(&mut self) -> Vec<IncomingIpc> {
        let mut incoming = Vec::new();
        let mut dead = Vec::new();

        for (&id, client) in self.clients.iter_mut() {
            match client.read_messages() {
                Ok(lines) => {
                    for line in lines {
                        match serde_json::from_str::<IpcMessage>(&line) {
                            Ok(IpcMessage::Command(cmd)) => {
                                incoming.push(IncomingIpc::Command {
                                    client_id: id,
                                    name: cmd.command,
                                    args: cmd.args,
                                });
                            }
                            Ok(IpcMessage::Query(q)) => {
                                incoming.push(IncomingIpc::Query {
                                    client_id: id,
                                    name: q.query,
                                    args: q.args,
                                });
                            }
                            Ok(IpcMessage::Subscribe(sub)) => {
                                incoming.push(IncomingIpc::Subscribe {
                                    client_id: id,
                                    topics: sub.subscribe,
                                });
                            }
                            Err(e) => {
                                warn!("[ipc] bad message from client {id}: {e}");
                                let _ = client.send_response(&IpcResponse::err(format!("parse error: {e}")));
                            }
                        }
                    }
                }
                Err(_) => dead.push(id),
            }
        }

        for id in dead {
            debug!("[ipc] client {} disconnected", id);
            self.clients.remove(&id);
        }

        incoming
    }

    /// Send a response to a specific client.
    pub fn respond(&mut self, client_id: u64, resp: &IpcResponse) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            if let Err(e) = client.send_response(resp) {
                warn!("[ipc] failed to send response to client {client_id}: {e}");
                self.clients.remove(&client_id);
            }
        }
    }

    /// Register subscriptions for a client.
    pub fn subscribe(&mut self, client_id: u64, topics: Vec<String>) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.subscriptions = topics;
        }
    }

    /// Broadcast an event to all subscribed clients.
    pub fn broadcast(&mut self, event: &IpcEvent) {
        let mut dead = Vec::new();
        for (&id, client) in self.clients.iter_mut() {
            if client.is_subscribed(&event.event) {
                if let Err(_) = client.send_event(event) {
                    dead.push(id);
                }
            }
        }
        for id in dead {
            self.clients.remove(&id);
        }
    }

    /// Clean shutdown: close all clients and remove the socket file.
    pub fn shutdown(&mut self) {
        self.clients.clear();
        let _ = std::fs::remove_file(&self.socket_path);
        info!("[ipc] server shut down");
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Helper: create an IpcServer bound to a unique temp path.
    fn make_test_server() -> IpcServer {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("jwm-ipc-test-{}-{}.sock", std::process::id(), id));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        IpcServer {
            listener,
            socket_path: path,
            clients: HashMap::new(),
            next_id: 1,
        }
    }

    #[test]
    fn accept_and_poll_command() {
        let mut server = make_test_server();
        let path = server.socket_path.clone();

        // Connect a client and send a command
        let mut client = UnixStream::connect(&path).unwrap();
        client.write_all(b"{\"command\":\"killclient\",\"args\":null}\n").unwrap();

        // Give the OS a moment
        std::thread::sleep(std::time::Duration::from_millis(20));

        server.accept_connections();
        let msgs = server.poll_clients();
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            IncomingIpc::Command { name, .. } => assert_eq!(name, "killclient"),
            _ => panic!("expected Command"),
        }
    }

    #[test]
    fn respond_to_client() {
        let mut server = make_test_server();
        let path = server.socket_path.clone();

        let mut client = UnixStream::connect(&path).unwrap();
        client.set_nonblocking(false).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        client.write_all(b"{\"query\":\"get_version\"}\n").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));

        server.accept_connections();
        let msgs = server.poll_clients();
        assert_eq!(msgs.len(), 1);

        // Respond
        match &msgs[0] {
            IncomingIpc::Query { client_id, .. } => {
                let resp = crate::ipc::IpcResponse::ok(Some(serde_json::json!({"v": "0.2"})));
                server.respond(*client_id, &resp);
            }
            _ => panic!("expected Query"),
        }

        // Client reads the response
        let mut buf = [0u8; 1024];
        let n = std::io::Read::read(&mut client, &mut buf).unwrap();
        let line = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(line.contains("\"success\":true"));
    }

    #[test]
    fn broadcast_to_subscriber() {
        let mut server = make_test_server();
        let path = server.socket_path.clone();

        let mut c1 = UnixStream::connect(&path).unwrap();
        c1.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        c1.write_all(b"{\"subscribe\":[\"window\"]}\n").unwrap();

        let mut c2 = UnixStream::connect(&path).unwrap();
        c2.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        // c2 doesn't subscribe

        std::thread::sleep(std::time::Duration::from_millis(20));

        server.accept_connections();
        let msgs = server.poll_clients();

        // Process subscribe
        for msg in &msgs {
            if let IncomingIpc::Subscribe { client_id, topics } = msg {
                server.subscribe(*client_id, topics.clone());
                server.respond(*client_id, &crate::ipc::IpcResponse::ok(None));
            }
        }

        // Read the subscribe confirmation from c1
        let mut buf = [0u8; 1024];
        let _ = std::io::Read::read(&mut c1, &mut buf).unwrap();

        // Broadcast
        let event = crate::ipc::IpcEvent {
            event: "window/new".to_string(),
            payload: serde_json::json!({"id": 42}),
        };
        server.broadcast(&event);

        // c1 should receive the event
        let mut buf = [0u8; 1024];
        let n = std::io::Read::read(&mut c1, &mut buf).unwrap();
        let line = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(line.contains("window/new"));

        // c2 should NOT receive (no subscription, and read would block/timeout)
        c2.set_read_timeout(Some(std::time::Duration::from_millis(100))).unwrap();
        let result = std::io::Read::read(&mut c2, &mut [0u8; 1024]);
        assert!(result.is_err() || result.unwrap() == 0);
    }

    #[test]
    fn disconnected_client_is_cleaned() {
        let mut server = make_test_server();
        let path = server.socket_path.clone();

        let client = UnixStream::connect(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        server.accept_connections();
        assert_eq!(server.clients.len(), 1);

        // Drop the client (disconnect)
        drop(client);
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Polling should detect the disconnect
        let _ = server.poll_clients();
        assert_eq!(server.clients.len(), 0);
    }
}
