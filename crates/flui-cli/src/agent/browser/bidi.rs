//! The link in WebDriver BiDi: a session on the browser's WebSocket, the tab it opened with, and
//! a channel the page reports through. A channel is handed to a function as an argument, so
//! the page gets no global of the link's.

use std::time::Instant;

use serde_json::{Map, Value, json};

use super::socket::Socket;
use super::{BrowserLink, COMMAND_TIMEOUT, LinkError, LinkEvent, Remote};

/// The channel every report comes back on.
const CHANNEL: &str = "flui-agent";

/// The events the link reads.
const EVENTS: [&str; 3] = [
    "script.message",
    "browsingContext.navigationStarted",
    "browsingContext.contextDestroyed",
];

/// A tab of a browser that speaks WebDriver BiDi.
pub(super) struct Bidi {
    socket: Socket,
    /// The tab's top-level browsing context.
    context: String,
}

impl Bidi {
    /// Starts a session on `socket` and takes the browser's first tab.
    pub(super) fn attach(socket: Socket) -> Result<Self, LinkError> {
        let mut link = Self {
            socket,
            context: String::new(),
        };
        link.command("session.new", json!({ "capabilities": {} }))?;
        let tree = link.command("browsingContext.getTree", json!({ "maxDepth": 0 }))?;
        let first = tree
            .get("contexts")
            .and_then(Value::as_array)
            .and_then(|contexts| contexts.first())
            .and_then(|context| context.get("context"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        link.context = match first {
            Some(context) => context,
            None => link
                .command("browsingContext.create", json!({ "type": "tab" }))?
                .get("context")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| LinkError::Refused("the browser opened no tab".into()))?,
        };
        link.command("session.subscribe", json!({ "events": EVENTS }))?;
        Ok(link)
    }

    /// Sends `method` and returns its result.
    fn command(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        let mut message = Map::new();
        message.insert("method".into(), Value::from(method));
        message.insert("params".into(), params);
        let mut answer = self
            .socket
            .request(message, Instant::now() + COMMAND_TIMEOUT)?;
        if answer.get("type").and_then(Value::as_str) == Some("success") {
            return Ok(answer
                .get_mut("result")
                .map(Value::take)
                .unwrap_or_default());
        }
        let error = answer
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("error");
        let message = answer.get("message").and_then(Value::as_str).unwrap_or("");
        Err(LinkError::Refused(format!("{method}: {error}: {message}")))
    }

    fn call_function(&mut self, params: Value) -> Result<Value, LinkError> {
        let mut evaluated = self.command("script.callFunction", params)?;
        if evaluated.get("type").and_then(Value::as_str) == Some("success") {
            return Ok(evaluated
                .get_mut("result")
                .map(Value::take)
                .unwrap_or_default());
        }
        let text = evaluated
            .pointer("/exceptionDetails/text")
            .and_then(Value::as_str)
            .unwrap_or("an exception");
        Err(LinkError::Threw(text.to_owned()))
    }
}

/// `args` as BiDi's local values.
fn arguments(args: &[&str]) -> Vec<Value> {
    args.iter()
        .map(|arg| json!({ "type": "string", "value": arg }))
        .collect()
}

impl BrowserLink for Bidi {
    fn open(&mut self, preload: &str, url: &str) -> Result<(), LinkError> {
        self.command(
            "script.addPreloadScript",
            json!({ "functionDeclaration": preload, "contexts": [self.context] }),
        )?;
        self.command(
            "browsingContext.navigate",
            json!({ "context": self.context, "url": url, "wait": "none" }),
        )?;
        Ok(())
    }

    fn call(&mut self, function: &str, args: &[&str]) -> Result<Option<Remote>, LinkError> {
        let mut arguments = arguments(args);
        arguments.push(json!({ "type": "channel", "value": { "channel": CHANNEL } }));
        let result = self.call_function(json!({
            "functionDeclaration": function,
            "awaitPromise": true,
            "target": { "context": self.context },
            "arguments": arguments,
            "resultOwnership": "root",
        }))?;
        Ok(result
            .get("handle")
            .and_then(Value::as_str)
            .map(|handle| Remote(handle.to_owned())))
    }

    fn call_on(&mut self, remote: &Remote, function: &str, args: &[&str]) -> Result<(), LinkError> {
        self.call_function(json!({
            "functionDeclaration": function,
            "awaitPromise": true,
            "target": { "context": self.context },
            "this": { "handle": remote.0 },
            "arguments": arguments(args),
            "resultOwnership": "none",
        }))
        .map(drop)
    }

    fn release(&mut self, remote: Remote) {
        let _ = self.command(
            "script.disown",
            json!({ "handles": [remote.0], "target": { "context": self.context } }),
        );
    }

    fn next_event(&mut self, deadline: Instant) -> Result<Option<LinkEvent>, LinkError> {
        loop {
            let Some(event) = self.socket.next_event(deadline)? else {
                return Ok(None);
            };
            let params = event.get("params").unwrap_or(&Value::Null);
            let ours = params.get("context").and_then(Value::as_str) == Some(&self.context);
            match event.get("method").and_then(Value::as_str) {
                Some("script.message")
                    if params.get("channel").and_then(Value::as_str) == Some(CHANNEL) =>
                {
                    if let Some(text) = params.pointer("/data/value").and_then(Value::as_str) {
                        return Ok(Some(LinkEvent::Report(text.to_owned())));
                    }
                }
                Some("browsingContext.navigationStarted") if ours => {
                    return Ok(Some(LinkEvent::Navigated));
                }
                Some("browsingContext.contextDestroyed") if ours => {
                    return Err(LinkError::Gone("its tab was closed".into()));
                }
                _ => {}
            }
        }
    }
}
