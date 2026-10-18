//! A WebDriver BiDi session in headless Chrome, through ChromeDriver.
//!
//! ChromeDriver opens the session over classic WebDriver HTTP with
//! `webSocketUrl` requested; every command after that is BiDi JSON over the
//! returned WebSocket ([`Session::command`]). Input is `input.performActions`,
//! which Chrome delivers as trusted input through its own input pipeline, so
//! the page sees the events a user's devices would produce (with user
//! activation), unlike events a script constructs.

use std::io::{BufRead as _, BufReader};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// How long ChromeDriver gets to report its port, and a command its reply.
const STARTUP: Duration = Duration::from_secs(30);

/// A running ChromeDriver, stopped when dropped.
pub(super) struct Driver {
    child: Child,
    port: u16,
}

impl Driver {
    /// Start `driver` on a free port.
    pub(super) fn start(driver: &Path) -> anyhow::Result<Self> {
        let mut child = Command::new(driver)
            .arg("--port=0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", driver.display()))?;
        let stdout = child.stdout.take().context("ChromeDriver's stdout")?;
        let (sender, receiver) = std::sync::mpsc::channel();
        // The rest of its output is read and dropped, so it never blocks on
        // a full pipe.
        std::thread::spawn(move || {
            let mut sender = Some(sender);
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(port) = line
                    .split_once("started successfully on port ")
                    .and_then(|(_, rest)| rest.trim_end_matches('.').trim().parse::<u16>().ok())
                    && let Some(sender) = sender.take()
                {
                    let _ = sender.send(port);
                }
            }
        });
        let Ok(port) = receiver.recv_timeout(STARTUP) else {
            let _ = child.kill();
            bail!("ChromeDriver did not report its port");
        };
        Ok(Self { child, port })
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One BiDi session with one browsing context, ended when dropped.
pub(super) struct Session {
    base: String,
    id: String,
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
    context: String,
}

impl Session {
    /// Open a headless Chrome session through `driver`, `width` by `height`
    /// CSS pixels at a device pixel ratio of one.
    pub(super) fn open(driver: &Driver, width: u32, height: u32) -> anyhow::Result<Self> {
        let base = format!("http://127.0.0.1:{}", driver.port);
        let capabilities = json!({
            "capabilities": {
                "alwaysMatch": {
                    "browserName": "chrome",
                    "webSocketUrl": true,
                    "goog:chromeOptions": {
                        "args": [
                            "--headless=new",
                            format!("--window-size={width},{height}"),
                            "--force-device-scale-factor=1",
                            "--no-first-run",
                            "--no-default-browser-check",
                        ],
                    },
                },
            },
        });
        let mut response = ureq::post(format!("{base}/session"))
            .send_json(&capabilities)
            .context("ChromeDriver refused a new session")?;
        let created: Value = response
            .body_mut()
            .read_json()
            .context("reading ChromeDriver's new session")?;
        let id = created["value"]["sessionId"]
            .as_str()
            .context("a session id")?
            .to_owned();
        let socket_url = created["value"]["capabilities"]["webSocketUrl"]
            .as_str()
            .context("ChromeDriver offered no BiDi socket")?
            .to_owned();
        let (socket, _) = tungstenite::connect(socket_url.as_str())
            .with_context(|| format!("connecting to {socket_url}"))?;
        let mut session = Self {
            base,
            id,
            socket,
            next: 1,
            context: String::new(),
        };
        let tree = session.command("browsingContext.getTree", json!({}))?;
        tree["contexts"][0]["context"]
            .as_str()
            .context("the session's browsing context")?
            .clone_into(&mut session.context);
        Ok(session)
    }

    /// Send one command and wait for its result, skipping events.
    pub(super) fn command(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next;
        self.next += 1;
        let message = json!({ "id": id, "method": method, "params": params });
        self.socket
            .send(Message::text(message.to_string()))
            .with_context(|| format!("sending {method}"))?;
        let deadline = Instant::now() + STARTUP;
        loop {
            if Instant::now() > deadline {
                bail!("{method}: no reply");
            }
            let Message::Text(text) = self
                .socket
                .read()
                .with_context(|| format!("{method}: reading its reply"))?
            else {
                continue;
            };
            let reply: Value = serde_json::from_str(&text).context("a BiDi message")?;
            if reply["id"].as_u64() != Some(id) {
                continue;
            }
            if reply["type"] == "error" {
                bail!(
                    "{method}: {} {}",
                    reply["error"].as_str().unwrap_or("error"),
                    reply["message"].as_str().unwrap_or_default()
                );
            }
            return Ok(reply["result"].clone());
        }
    }

    /// Load `url` and wait for it to finish loading.
    pub(super) fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let context = self.context.clone();
        self.command(
            "browsingContext.navigate",
            json!({ "context": context, "url": url, "wait": "complete" }),
        )?;
        Ok(())
    }

    /// Evaluate `expression` in the page, awaiting a promise, and return its
    /// value as BiDi serializes it (`{"type": ..., "value": ...}`).
    pub(super) fn evaluate(&mut self, expression: &str) -> anyhow::Result<Value> {
        let context = self.context.clone();
        let result = self.command(
            "script.evaluate",
            json!({
                "expression": expression,
                "target": { "context": context },
                "awaitPromise": true,
                "userActivation": false,
            }),
        )?;
        if result["type"] == "exception" {
            bail!(
                "{expression}: {}",
                result["exceptionDetails"]["text"]
                    .as_str()
                    .unwrap_or("threw")
            );
        }
        Ok(result["result"].clone())
    }

    /// Evaluate `expression` for a string.
    pub(super) fn string(&mut self, expression: &str) -> anyhow::Result<String> {
        let value = self.evaluate(expression)?;
        value["value"]
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("{expression}: not a string but {value}"))
    }

    /// Perform input `actions` (the `input.performActions` source list) and
    /// release whatever they left held.
    pub(super) fn perform(&mut self, actions: Value) -> anyhow::Result<()> {
        let context = self.context.clone();
        self.command(
            "input.performActions",
            json!({ "context": context, "actions": actions }),
        )?;
        self.command("input.releaseActions", json!({ "context": context }))?;
        Ok(())
    }

    /// A Chrome DevTools command on the page, through ChromeDriver's
    /// `goog:cdp` extension, for the input BiDi does not define: an input
    /// method's composition (`Input.imeSetComposition`, `Input.insertText`),
    /// which Chrome delivers through the same path as an OS input method's.
    pub(super) fn devtools(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let context = self.context.clone();
        let session = self.command("goog:cdp.getSession", json!({ "context": context }))?;
        let session = session["session"]
            .as_str()
            .context("a DevTools session")?
            .to_owned();
        self.command(
            "goog:cdp.sendCommand",
            json!({ "method": method, "params": params, "session": session }),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.socket.close(None);
        let _ = ureq::delete(format!("{}/session/{}", self.base, self.id)).call();
    }
}
