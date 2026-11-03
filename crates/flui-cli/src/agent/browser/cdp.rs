//! The link in the Chrome DevTools protocol: a flat session attached to the browser's first
//! tab, and a binding the page reports through. A binding is a global function of the page,
//! the one name the link adds to it.

use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use super::socket::Socket;
use super::{BrowserLink, COMMAND_TIMEOUT, LinkError, LinkEvent, Remote};

/// The page's function that reports to the link.
const BINDING: &str = "__flui_agent_report";

/// How long the browser has to open its first tab.
const FIRST_TAB_WAIT: Duration = Duration::from_secs(10);

/// A tab of a browser that speaks the DevTools protocol.
pub(super) struct Cdp {
    socket: Socket,
    /// The flat session attached to the tab.
    session: String,
}

impl Cdp {
    /// Attaches to the first tab the browser on `socket` opens.
    pub(super) fn attach(mut socket: Socket) -> Result<Self, LinkError> {
        let opened = Instant::now() + FIRST_TAB_WAIT;
        let target = loop {
            let targets = command(&mut socket, None, "Target.getTargets", json!({}))?;
            let page = targets
                .get("targetInfos")
                .and_then(Value::as_array)
                .and_then(|targets| {
                    targets
                        .iter()
                        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
                })
                .and_then(|target| target.get("targetId"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(page) = page {
                break page;
            }
            if Instant::now() >= opened {
                return Err(LinkError::Refused("the browser opened no tab".into()));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let attached = command(
            &mut socket,
            None,
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
        )?;
        let session = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| LinkError::Refused("Target.attachToTarget named no session".into()))?;
        let mut link = Self { socket, session };
        link.command("Page.enable", json!({}))?;
        link.command("Runtime.enable", json!({}))?;
        link.command("Runtime.addBinding", json!({ "name": BINDING }))?;
        Ok(link)
    }

    fn command(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        command(&mut self.socket, Some(&self.session), method, params)
    }

    /// What a `Runtime` evaluation gave, or what it threw.
    fn evaluated(mut result: Value) -> Result<Value, LinkError> {
        if let Some(details) = result.get("exceptionDetails") {
            let text = details
                .pointer("/exception/description")
                .or_else(|| details.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("an exception");
            return Err(LinkError::Threw(text.to_owned()));
        }
        Ok(result
            .get_mut("result")
            .map(Value::take)
            .unwrap_or_default())
    }
}

/// Sends `method` on `session` (the browser's own when `None`) and returns its result.
fn command(
    socket: &mut Socket,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, LinkError> {
    let mut message = Map::new();
    message.insert("method".into(), Value::from(method));
    message.insert("params".into(), params);
    if let Some(session) = session {
        message.insert("sessionId".into(), Value::from(session));
    }
    let mut answer = socket.request(message, Instant::now() + COMMAND_TIMEOUT)?;
    if let Some(error) = answer.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error");
        return Err(LinkError::Refused(format!("{method}: {text}")));
    }
    Ok(answer
        .get_mut("result")
        .map(Value::take)
        .unwrap_or_default())
}

impl BrowserLink for Cdp {
    fn open(&mut self, preload: &str, url: &str) -> Result<(), LinkError> {
        self.command(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": format!("({preload})();") }),
        )?;
        self.command("Page.navigate", json!({ "url": url }))?;
        Ok(())
    }

    fn call(&mut self, function: &str, args: &[&str]) -> Result<Option<Remote>, LinkError> {
        // An argument can only be a value here, so the report function is made in the call.
        let arguments = args
            .iter()
            .map(|arg| Value::from(*arg).to_string())
            .chain([format!(
                "(report) => globalThis[{binding}](report)",
                binding = Value::from(BINDING)
            )])
            .collect::<Vec<String>>()
            .join(", ");
        let expression = format!("({function})({arguments})");
        let result = self.command(
            "Runtime.evaluate",
            json!({ "expression": expression, "awaitPromise": true }),
        )?;
        let result = Self::evaluated(result)?;
        Ok(result
            .get("objectId")
            .and_then(Value::as_str)
            .map(|object| Remote(object.to_owned())))
    }

    fn call_on(&mut self, remote: &Remote, function: &str, args: &[&str]) -> Result<(), LinkError> {
        let arguments = args
            .iter()
            .map(|arg| json!({ "value": arg }))
            .collect::<Vec<Value>>();
        let result = self.command(
            "Runtime.callFunctionOn",
            json!({
                "objectId": remote.0,
                "functionDeclaration": function,
                "arguments": arguments,
                "awaitPromise": true,
            }),
        )?;
        Self::evaluated(result).map(drop)
    }

    fn release(&mut self, remote: Remote) {
        let _ = self.command("Runtime.releaseObject", json!({ "objectId": remote.0 }));
    }

    fn next_event(&mut self, deadline: Instant) -> Result<Option<LinkEvent>, LinkError> {
        loop {
            let Some(event) = self.socket.next_event(deadline)? else {
                return Ok(None);
            };
            let params = event.get("params").unwrap_or(&Value::Null);
            let method = event.get("method").and_then(Value::as_str);
            if event.get("sessionId").and_then(Value::as_str) != Some(&self.session) {
                // The browser's own events: only the tab's detaching concerns the link.
                if method == Some("Target.detachedFromTarget")
                    && params.get("sessionId").and_then(Value::as_str) == Some(&self.session)
                {
                    return Err(LinkError::Gone("its tab was closed".into()));
                }
                continue;
            }
            match method {
                Some("Runtime.bindingCalled")
                    if params.get("name").and_then(Value::as_str) == Some(BINDING) =>
                {
                    if let Some(payload) = params.get("payload").and_then(Value::as_str) {
                        return Ok(Some(LinkEvent::Report(payload.to_owned())));
                    }
                }
                Some("Page.frameNavigated")
                    if params.pointer("/frame/parentId").is_none_or(Value::is_null) =>
                {
                    return Ok(Some(LinkEvent::Navigated));
                }
                Some("Inspector.detached") => {
                    return Err(LinkError::Gone("its tab was detached".into()));
                }
                _ => {}
            }
        }
    }
}
