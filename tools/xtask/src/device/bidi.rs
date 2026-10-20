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
    /// The page's console and error lines since the last
    /// [`Session::take_logs`], once [`Session::subscribe_logs`] ran.
    logs: Vec<String>,
    /// Chrome's own DevTools address (`host:port`), which answers while
    /// ChromeDriver waits on a command the page never acknowledges.
    debugger: Option<String>,
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
        // A reply that never comes fails its command rather than the run
        // hanging on the read.
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream
                .set_read_timeout(Some(STARTUP))
                .context("bounding the socket's reads")?;
        }
        let mut session = Self {
            base,
            id,
            socket,
            next: 1,
            context: String::new(),
            logs: Vec::new(),
            debugger: created["value"]["capabilities"]["goog:chromeOptions"]["debuggerAddress"]
                .as_str()
                .map(str::to_owned),
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
            if reply["method"] == "log.entryAdded" {
                let entry = &reply["params"];
                self.logs.push(format!(
                    "{} {}",
                    entry["level"].as_str().unwrap_or("log"),
                    entry["text"].as_str().unwrap_or_default()
                ));
                continue;
            }
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

    /// Start sampling the page's script into a trace, over Chrome's own
    /// DevTools socket: the V8 profiler starts on the page's thread now, and
    /// samples from a thread of its own, so a page that later sticks in a
    /// loop the debugger cannot interrupt still shows where.
    pub(super) fn start_profile(&self) -> anyhow::Result<Profile> {
        let address = self
            .debugger
            .clone()
            .context("Chrome reported no DevTools address")?;
        let version: Value = ureq::get(format!("http://{address}/json/version"))
            .call()
            .context("reading Chrome's DevTools version")?
            .body_mut()
            .read_json()?;
        let socket_url = version["webSocketDebuggerUrl"]
            .as_str()
            .context("no browser DevTools socket")?;
        let (socket, _) = tungstenite::connect(socket_url)
            .with_context(|| format!("connecting to {socket_url}"))?;
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(STARTUP))?;
        }
        let mut profile = Profile { socket, next: 1 };
        profile.call(
            "Tracing.start",
            json!({
                "traceConfig": {
                    "includedCategories": ["disabled-by-default-v8.cpu_profiler"],
                    "recordMode": "recordContinuously",
                },
                "transferMode": "ReturnAsStream",
            }),
        )?;
        Ok(profile)
    }

    /// Collect the page's console and uncaught errors (`log.entryAdded`)
    /// from now on, for [`Self::take_logs`].
    pub(super) fn subscribe_logs(&mut self) -> anyhow::Result<()> {
        self.command("session.subscribe", json!({ "events": ["log.entryAdded"] }))?;
        Ok(())
    }

    /// The page's log lines collected since the last call.
    pub(super) fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// Size the page's viewport to `width` by `height` CSS pixels at a
    /// device pixel ratio of one, whatever the window's frame takes from the
    /// window size (`browsingContext.setViewport`).
    pub(super) fn set_viewport(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        let context = self.context.clone();
        self.command(
            "browsingContext.setViewport",
            json!({
                "context": context,
                "viewport": { "width": width, "height": height },
                "devicePixelRatio": 1,
            }),
        )?;
        Ok(())
    }

    /// The page's viewport as Chrome composited it, a PNG
    /// (`browsingContext.captureScreenshot`).
    pub(super) fn capture(&mut self) -> anyhow::Result<Vec<u8>> {
        use base64::Engine as _;
        let context = self.context.clone();
        let shot = self.command(
            "browsingContext.captureScreenshot",
            json!({ "context": context, "origin": "viewport" }),
        )?;
        let data = shot["data"].as_str().context("a screenshot's data")?;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("decoding a screenshot")
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

/// A trace of the page's script samples, from [`Session::start_profile`].
pub(super) struct Profile {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
}

impl Profile {
    /// One browser DevTools command; its events are skipped.
    fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next;
        self.next += 1;
        self.socket.send(Message::text(
            json!({ "id": id, "method": method, "params": params }).to_string(),
        ))?;
        loop {
            let reply = self.message(method)?;
            if reply["id"].as_u64() == Some(id) {
                if let Some(error) = reply.get("error") {
                    bail!("{method}: {error}");
                }
                return Ok(reply["result"].clone());
            }
        }
    }

    fn message(&mut self, method: &str) -> anyhow::Result<Value> {
        loop {
            if let Message::Text(text) = self
                .socket
                .read()
                .with_context(|| format!("{method}: reading DevTools"))?
            {
                return serde_json::from_str(&text).context("a DevTools message");
            }
        }
    }

    /// End the trace and name where the samples since its start fell: the
    /// functions sampled most, innermost frame first, and the stack of the
    /// most sampled one.
    pub(super) fn hottest(mut self) -> anyhow::Result<Vec<String>> {
        self.call("Tracing.end", json!({}))?;
        let stream = loop {
            let message = self.message("Tracing.end")?;
            if message["method"] == "Tracing.tracingComplete" {
                break message["params"]["stream"]
                    .as_str()
                    .context("a trace stream")?
                    .to_owned();
            }
        };
        let mut trace = String::new();
        loop {
            let chunk = self.call("IO.read", json!({ "handle": stream }))?;
            trace.push_str(chunk["data"].as_str().unwrap_or_default());
            if chunk["eof"] == true {
                break;
            }
        }
        let trace: Value =
            serde_json::from_str(&without_lone_surrogates(&trace)).context("the trace")?;
        let events = trace["traceEvents"]
            .as_array()
            .or_else(|| trace.as_array())
            .context("trace events")?;
        // Nodes arrive in chunks, each naming its frame and parent once.
        let mut frames: std::collections::HashMap<u64, (String, Option<u64>)> =
            std::collections::HashMap::new();
        let mut samples: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for event in events
            .iter()
            .filter(|event| event["name"] == "ProfileChunk")
        {
            let profile = &event["args"]["data"]["cpuProfile"];
            for node in profile["nodes"].as_array().into_iter().flatten() {
                let Some(id) = node["id"].as_u64() else {
                    continue;
                };
                let frame = &node["callFrame"];
                let name = match frame["functionName"].as_str() {
                    Some("") | None => "(anonymous)".to_owned(),
                    Some(name) => name.to_owned(),
                };
                frames.insert(id, (name, node["parent"].as_u64()));
            }
            for sample in profile["samples"].as_array().into_iter().flatten() {
                if let Some(id) = sample.as_u64() {
                    *samples.entry(id).or_default() += 1;
                }
            }
        }
        // The profiler's own buckets say the page was not running script.
        let idle = |id: &u64| {
            frames.get(id).is_some_and(|(name, _)| {
                matches!(
                    name.as_str(),
                    "(idle)" | "(program)" | "(root)" | "(garbage collector)"
                )
            })
        };
        let mut hot: Vec<(u64, u64)> = samples.into_iter().filter(|(id, _)| !idle(id)).collect();
        hot.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        let name = |id: u64| frames.get(&id).map_or("?", |(name, _)| name.as_str());
        let mut lines: Vec<String> = hot
            .iter()
            .take(8)
            .map(|&(id, count)| format!("{count:>6} samples in {}", name(id)))
            .collect();
        // Inclusive counts: the samples each function was on the stack for,
        // which tells one long call from many short ones.
        let mut inclusive: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
        for &(leaf, count) in &hot {
            let mut seen = std::collections::HashSet::new();
            let mut id = Some(leaf);
            while let Some(at) = id {
                let function = name(at);
                if seen.insert(function) {
                    *inclusive.entry(function).or_default() += count;
                }
                id = frames.get(&at).and_then(|(_, parent)| *parent);
            }
        }
        let mut inclusive: Vec<(&str, u64)> = inclusive.into_iter().collect();
        inclusive.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        lines.push("on the stack for:".to_owned());
        lines.extend(
            inclusive
                .iter()
                .take(40)
                .map(|(function, count)| format!("{count:>6} {function}")),
        );
        if let Some(&(mut id, _)) = hot.first() {
            lines.push("the most sampled stack, innermost first:".to_owned());
            for _ in 0..80 {
                lines.push(format!("  {}", name(id)));
                match frames.get(&id).and_then(|(_, parent)| *parent) {
                    Some(parent) => id = parent,
                    None => break,
                }
            }
        }
        Ok(lines)
    }
}

/// `json` with each `\u` escape of a lone UTF-16 surrogate replaced by
/// U+FFFD: Chrome's traces cut function names mid-pair, which JSON parsers
/// refuse.
fn without_lone_surrogates(json: &str) -> String {
    fn escape_at(bytes: &[u8], at: usize) -> Option<u16> {
        let digits = bytes.get(at + 2..at + 6)?;
        if bytes.get(at) != Some(&b'\\') || bytes.get(at + 1) != Some(&b'u') {
            return None;
        }
        u16::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }
    let bytes = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    let mut copied = 0;
    let mut at = 0;
    let mut escaped = false;
    while at < bytes.len() {
        if escaped {
            escaped = false;
            if bytes[at] == b'u'
                && let Some(unit) = escape_at(bytes, at - 1)
            {
                let high = (0xD800..=0xDBFF).contains(&unit);
                let low = (0xDC00..=0xDFFF).contains(&unit);
                let paired = high
                    && escape_at(bytes, at + 5)
                        .is_some_and(|next| (0xDC00..=0xDFFF).contains(&next));
                if paired {
                    at += 11;
                    continue;
                }
                if high || low {
                    out.push_str(&json[copied..at - 1]);
                    out.push_str("\\ufffd");
                    at += 5;
                    copied = at;
                    continue;
                }
            }
        } else if bytes[at] == b'\\' {
            escaped = true;
        }
        at += 1;
    }
    out.push_str(&json[copied..]);
    out
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.socket.close(None);
        let _ = ureq::delete(format!("{}/session/{}", self.base, self.id)).call();
    }
}

#[cfg(test)]
mod tests {
    use super::without_lone_surrogates;

    #[test]
    fn a_trace_keeps_its_surrogate_pairs_and_loses_its_lone_halves() {
        // A pair spelled out of two escapes, as Chrome writes one.
        let pair = ["\\u", "d83d", "\\u", "de00"].concat();
        let json = &format!(r#"["{pair}", "cut \ud83d", "\ude00 alone", "\\ud83d is text", "A"]"#);
        let value: Vec<String> =
            serde_json::from_str(&without_lone_surrogates(json)).expect("valid JSON");
        assert_eq!(
            value,
            [
                "\u{1F600}",
                "cut \u{FFFD}",
                "\u{FFFD} alone",
                r"\ud83d is text",
                "A"
            ]
        );
    }
}
