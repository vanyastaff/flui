//! A stand-in for a ChromeDriver's WebDriver BiDi session, on a loopback WebSocket.
//!
//! [`ScriptedBrowser`] serves one connection, as `Driver::attach_page` makes it, and plays a tab
//! whose page defines an agent export: a `script.callFunction` that connects to the export
//! answers a handle, each line sent on it is answered by a [`Script`], as a [`super::ScriptedAgent`]
//! answers one, and every line the agent writes reaches the driver as a `script.message` on the
//! channel the driver connected with. The canvas sits at the viewport's top-left at 800 by 600 at
//! a device pixel ratio of one, input commands are acknowledged, and a [`BrowserScript`] may
//! answer any command otherwise. Every command is recorded in a [`BrowserLog`].

use std::net::TcpListener;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use flui_protocol::envelope::{Greeting, Request, ServerLine};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use super::{Reply, Script, scripted_hello};
use crate::error::Result;
use crate::lane::chrome::bidi::MAX_MESSAGE;

/// The tab the stand-in plays.
const CONTEXT: &str = "tab-1";

/// The handle of the page's connection object.
const CONNECTION: &str = "page-connection";

/// How a [`BrowserScript`] answers one command.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BrowserReply {
    /// As the stand-in plays it.
    Played,
    /// With this result.
    Result(Value),
    /// With this BiDi error.
    Error {
        /// BiDi's error code.
        error: String,
        /// The message.
        message: String,
    },
    /// Never.
    Silent,
    /// With an answer to an earlier command's id first, then as the stand-in plays it.
    StaleFirst,
    /// With a message one byte past what the driver reads.
    Oversized,
}

/// The answer to each BiDi command, by its method and parameters.
pub type BrowserScript = Box<dyn FnMut(&str, &Value) -> BrowserReply + Send>;

/// The commands a [`ScriptedBrowser`] received, read from the test's side.
#[derive(Debug, Clone, Default)]
pub struct BrowserLog {
    commands: Arc<Mutex<Vec<(String, Value)>>>,
}

impl BrowserLog {
    /// Every command, method and parameters, in the order received.
    #[must_use]
    pub fn commands(&self) -> Vec<(String, Value)> {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The parameters of each command named `method`, in order.
    #[must_use]
    pub fn of(&self, method: &str) -> Vec<Value> {
        self.commands()
            .into_iter()
            .filter(|(name, _)| name == method)
            .map(|(_, params)| params)
            .collect()
    }

    fn push(&self, method: &str, params: &Value) {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((method.to_owned(), params.clone()));
    }
}

/// A stand-in for a BiDi session: see the [module docs](self).
pub struct ScriptedBrowser {
    url: String,
    token: String,
    log: BrowserLog,
    thread: Option<JoinHandle<()>>,
}

impl ScriptedBrowser {
    /// Serve one connection on a free loopback port, its page's agent serving `methods` and
    /// answering by `agent`, every other command answered by `browser`.
    ///
    /// # Errors
    ///
    /// When no port can be bound.
    pub fn start(methods: &[&str], agent: Script, browser: BrowserScript) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("ws://{}/session/scripted", listener.local_addr()?);
        let token = crate::launch::launch_token()?;
        let log = BrowserLog::default();
        let mut page = Page {
            token: token.clone(),
            methods: methods.iter().map(|method| (*method).to_owned()).collect(),
            agent,
            browser,
            greeted: false,
            channel: None,
            log: log.clone(),
        };
        let thread = thread::Builder::new()
            .name("flui-driver-scripted-browser".into())
            .spawn(move || {
                let Some(Ok(stream)) = listener.incoming().next() else {
                    return;
                };
                let Ok(mut ws) = tungstenite::accept(stream) else {
                    return;
                };
                while let Ok(message) = ws.read() {
                    match message {
                        Message::Text(text) => {
                            if page.heard(&mut ws, text.as_str()).is_err() {
                                return;
                            }
                        }
                        Message::Close(_) => return,
                        _ => {}
                    }
                }
            })?;
        Ok(Self {
            url,
            token,
            log,
            thread: Some(thread),
        })
    }

    /// The session's WebSocket, as a driver attaches to it.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The token the page's agent admits.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// What it received.
    #[must_use]
    pub fn log(&self) -> BrowserLog {
        self.log.clone()
    }

    /// The result a `script.callFunction` of the lane's canvas lookup answers: the canvas at
    /// (`left`, `top`) of `width` by `height` CSS pixels, at device pixel `ratio`.
    #[must_use]
    pub fn canvas(left: f64, top: f64, width: f64, height: f64, ratio: f64) -> Value {
        let number = |value: f64| json!({ "type": "number", "value": value });
        json!({
            "type": "success",
            "realm": "realm-1",
            "result": {
                "type": "array",
                "value": [number(left), number(top), number(width), number(height), number(ratio)],
            },
        })
    }
}

impl Drop for ScriptedBrowser {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take()
            && thread.is_finished()
        {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for ScriptedBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedBrowser")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// The tab and its page, as the stand-in plays them.
struct Page {
    token: String,
    methods: Vec<String>,
    agent: Script,
    browser: BrowserScript,
    greeted: bool,
    /// The channel the driver connected with.
    channel: Option<String>,
    log: BrowserLog,
}

type Socket = WebSocket<std::net::TcpStream>;

impl Page {
    fn heard(&mut self, ws: &mut Socket, text: &str) -> tungstenite::Result<()> {
        let Ok(command) = serde_json::from_str::<Value>(text) else {
            return Ok(());
        };
        let id = command.get("id").and_then(Value::as_u64).unwrap_or(0);
        let method = command
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let params = command.get("params").cloned().unwrap_or(Value::Null);
        self.log.push(&method, &params);
        match (self.browser)(&method, &params) {
            BrowserReply::Played => self.play(ws, id, &method, &params),
            BrowserReply::Result(result) => send(ws, &success(id, result)),
            BrowserReply::Error { error, message } => send(
                ws,
                &json!({ "type": "error", "id": id, "error": error, "message": message }),
            ),
            BrowserReply::Silent => Ok(()),
            BrowserReply::StaleFirst => {
                let stale = if id > 1 { id - 1 } else { u64::MAX };
                send(ws, &success(stale, json!({ "stale": true })))?;
                self.play(ws, id, &method, &params)
            }
            BrowserReply::Oversized => {
                ws.send(Message::text("x".repeat(MAX_MESSAGE + 1)))?;
                Ok(())
            }
        }
    }

    fn play(
        &mut self,
        ws: &mut Socket,
        id: u64,
        method: &str,
        params: &Value,
    ) -> tungstenite::Result<()> {
        let mut lines: Vec<Option<String>> = Vec::new();
        let result = match method {
            "browsingContext.getTree" => {
                json!({ "contexts": [{ "context": CONTEXT, "url": "about:blank", "children": [] }] })
            }
            "goog:cdp.getSession" => json!({ "session": "devtools-1" }),
            "script.callFunction" => {
                let function = params
                    .get("functionDeclaration")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if function.contains("page.connect") {
                    self.channel = params
                        .pointer("/arguments/1/value/channel")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    json!({
                        "type": "success",
                        "realm": "realm-1",
                        "result": { "type": "object", "handle": CONNECTION },
                    })
                } else if function.contains("getBoundingClientRect") {
                    ScriptedBrowser::canvas(0.0, 0.0, 800.0, 600.0, 1.0)
                } else {
                    if function.contains("this.send(line)") {
                        let line = params
                            .pointer("/arguments/0/value")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        lines = self.answer(line);
                    }
                    json!({ "type": "success", "realm": "realm-1", "result": { "type": "undefined" } })
                }
            }
            _ => json!({}),
        };
        send(ws, &success(id, result))?;
        let Some(channel) = self.channel.clone() else {
            return Ok(());
        };
        for line in lines {
            let data = match line {
                Some(line) => json!({ "type": "string", "value": line }),
                None => json!({ "type": "null" }),
            };
            send(
                ws,
                &json!({
                    "type": "event",
                    "method": "script.message",
                    "params": { "channel": channel, "data": data, "source": { "realm": "realm-1", "context": CONTEXT } },
                }),
            )?;
        }
        Ok(())
    }

    /// The lines the page's agent writes for `line`: `None` ends the connection.
    fn answer(&mut self, line: &str) -> Vec<Option<String>> {
        let text = |value: &ServerLine<Value>| serde_json::to_string(value).ok();
        if !self.greeted {
            let presented = serde_json::from_str::<Greeting>(line)
                .is_ok_and(|greeting| greeting.hello.token == self.token);
            if !presented {
                return vec![None];
            }
            self.greeted = true;
            return vec![text(&scripted_hello(&self.methods))];
        }
        let Ok(request) = serde_json::from_str::<Request<Value>>(line) else {
            return vec![None];
        };
        match (self.agent)(&request.method, &request.params) {
            Reply::Result(result) => vec![text(&ServerLine::answer(request.id, Ok(result)))],
            Reply::Error(error) => vec![text(&ServerLine::answer(request.id, Err(error)))],
            Reply::ResultThenClose(result) => {
                vec![text(&ServerLine::answer(request.id, Ok(result))), None]
            }
            Reply::ResultThenLines { result, lines, .. } => {
                std::iter::once(text(&ServerLine::answer(request.id, Ok(result))))
                    .chain(lines.iter().map(|line| serde_json::to_string(line).ok()))
                    .collect()
            }
            Reply::ResultThenBytes { result, .. } => {
                vec![text(&ServerLine::answer(request.id, Ok(result)))]
            }
            Reply::Close => vec![None],
        }
    }
}

fn success(id: u64, result: Value) -> Value {
    json!({ "type": "success", "id": id, "result": result })
}

fn send(ws: &mut Socket, message: &Value) -> tungstenite::Result<()> {
    ws.send(Message::text(message.to_string()))
}
