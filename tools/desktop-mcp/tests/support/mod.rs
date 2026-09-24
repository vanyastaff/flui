//! A minimal MCP client over the server's stdio: newline-delimited JSON-RPC,
//! as the stdio transport specifies.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// The protocol version the client asks for.
pub const PROTOCOL: &str = "2025-11-25";

/// A running server and the client end of its pipes.
pub struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Client {
    /// Spawns the server binary and completes the `initialize` handshake,
    /// returning the client and the `initialize` result.
    pub fn start() -> (Self, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_flui-desktop-mcp"))
            .env("RUST_LOG", "warn,xcap=off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("BUG: the server binary starts");
        let stdin = child.stdin.take().expect("BUG: stdin is piped");
        let stdout = child.stdout.take().expect("BUG: stdout is piped");
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            stdin,
            lines,
            next_id: 1,
        };
        let init = client.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": { "name": "desktop-mcp-test", "version": "0" },
            }),
        );
        client.notify("notifications/initialized", json!({}));
        (client, init)
    }

    fn send(&mut self, message: &Value) {
        let mut text = message.to_string();
        text.push('\n');
        self.stdin
            .write_all(text.as_bytes())
            .and_then(|()| self.stdin.flush())
            .expect("BUG: the server reads its stdin");
    }

    /// Sends a notification (no reply).
    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Sends a request and returns its `result`, panicking on an error reply.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("BUG: the server replies within 60 s");
            let message: Value =
                serde_json::from_str(&line).expect("BUG: the server writes JSON lines");
            if message.get("id") == Some(&json!(id)) {
                if let Some(error) = message.get("error") {
                    panic!("{method} failed: {error}");
                }
                return message["result"].clone();
            }
        }
    }

    /// Calls a tool and returns its result (which may carry `isError`).
    pub fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
