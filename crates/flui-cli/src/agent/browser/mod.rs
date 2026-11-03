//! `flui run --agent` in a browser (ADR-0288 §6).
//!
//! A page has no socket a client could reach, so this command reaches the page for it: it
//! starts the browser with the browser's own remote protocol on ([`launch`]), installs the
//! page's launch object before the app's module runs, and relays each connection `flui mcp`
//! makes to the session's usual local endpoint into a connection of the export the app defines
//! in the page ([`relay`]). `flui mcp` does not know it talks to a page.
//!
//! The browser is reached through a [`BrowserLink`]: the few things the relay asks of a tab,
//! spoken in the protocol the browser has without a separate driver ([`Dialect`]): WebDriver
//! BiDi in Firefox ([`bidi`]), the DevTools protocol in Chrome, Chromium, Edge, Brave and Opera
//! ([`cdp`]). Safari has neither without `safaridriver` (`docs/gaps.md` entry 42).

mod bidi;
mod cdp;
pub(crate) mod launch;
pub(crate) mod relay;
mod socket;

use std::fmt;
use std::time::{Duration, Instant};

use flui_protocol::launch::{PAGE_EXPORT_VERSION, PAGE_LAUNCH_GLOBAL};

/// How long the browser has to answer one command of the link.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// The global a page's app defines its agent's export as: the endpoint its launch names.
pub(crate) const PAGE_EXPORT: &str = "__flui_agent";

/// The remote protocol a browser speaks with no driver of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// WebDriver BiDi, over the WebSocket `--remote-debugging-port` opens (Firefox).
    Bidi,
    /// The Chrome DevTools protocol, over the same (Chromium and the browsers built on it).
    Cdp,
}

impl Dialect {
    /// The dialect of the browser `flui devices` lists as `id` (`browser:firefox`); `None` for
    /// one that has neither protocol without a driver of its own (Safari).
    pub(crate) fn of(id: &str) -> Option<Self> {
        match id.strip_prefix("browser:")? {
            "firefox" => Some(Self::Bidi),
            "chrome" | "chromium" | "edge" | "brave" | "opera" => Some(Self::Cdp),
            _ => None,
        }
    }
}

/// Why the link failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkError {
    /// The browser went, or its socket broke: the link is unusable afterwards.
    Gone(String),
    /// The browser refused a command.
    Refused(String),
    /// A function the link called threw in the page.
    Threw(String),
    /// The browser did not answer a command in time.
    Timeout,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gone(why) => write!(f, "the browser went: {why}"),
            Self::Refused(why) => write!(f, "the browser refused a command: {why}"),
            Self::Threw(why) => write!(f, "the page threw: {why}"),
            Self::Timeout => write!(
                f,
                "the browser did not answer within {} s",
                COMMAND_TIMEOUT.as_secs()
            ),
        }
    }
}

impl std::error::Error for LinkError {}

/// What a call left in the page, by the browser's reference to it. It lives as long as the
/// document, or until [`BrowserLink::release`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Remote(String);

/// What the tab tells the link of its own accord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkEvent {
    /// A string the page passed to the report function of a [`BrowserLink::call`].
    Report(String),
    /// The tab started on another document: everything the calls left in the page goes with
    /// the one it leaves.
    Navigated,
}

/// One tab of a browser, driven over the browser's remote protocol: what the relay asks of it.
pub(crate) trait BrowserLink: Send {
    /// Runs `preload`, a function declaration taking no argument, in every document the tab
    /// loads from now on before the document's own scripts, then loads `url`.
    ///
    /// # Errors
    ///
    /// Any [`LinkError`]; the tab may have started loading.
    fn open(&mut self, preload: &str, url: &str) -> Result<(), LinkError>;

    /// Calls `function`, a function declaration, in the tab's document with `args` and, last, a
    /// function that reports the string it is given as a [`LinkEvent::Report`]. Awaits the
    /// promise it returns, if it returns one, and keeps what it gives in the page: `None` when
    /// that is `null` or `undefined`.
    ///
    /// # Errors
    ///
    /// [`LinkError::Threw`] when it throws or its promise rejects.
    fn call(&mut self, function: &str, args: &[&str]) -> Result<Option<Remote>, LinkError>;

    /// Calls `function` with `remote` as `this` and `args`, awaiting the promise it returns.
    ///
    /// # Errors
    ///
    /// [`LinkError::Threw`] when it throws or its promise rejects.
    fn call_on(&mut self, remote: &Remote, function: &str, args: &[&str]) -> Result<(), LinkError>;

    /// Lets the page drop `remote`. A failure only leaves it to the document's end.
    fn release(&mut self, remote: Remote);

    /// The tab's next event, waiting no later than `deadline`; `None` once it has passed.
    ///
    /// # Errors
    ///
    /// [`LinkError::Gone`] once the browser or the tab went.
    fn next_event(&mut self, deadline: Instant) -> Result<Option<LinkEvent>, LinkError>;
}

/// The preload that installs the page's launch object: `values` are the launch's keys
/// (`flui_protocol::launch::ENDPOINT_EXTRA`, …) and their values. Only a top-level document is
/// given it: a frame inside the page is no app this session started.
pub(crate) fn preload(values: &[(&str, &str)]) -> String {
    let object = values
        .iter()
        .map(|&(key, value)| (key.to_owned(), serde_json::Value::from(value)))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    format!(
        "() => {{ if (globalThis.parent !== globalThis) {{ return; }} globalThis[{global}] = \
         {object}; }}",
        global = serde_json::Value::from(PAGE_LAUNCH_GLOBAL),
        object = serde_json::Value::Object(object),
    )
}

/// Connects to the page's export: called with the export's name, the connection's ID and the
/// report function. `null` while the page defines no export yet; it throws when the export
/// refuses the connection or speaks another version. Every line of the connection is reported
/// as `[id, line]`, and its end as `[id, null]`.
pub(crate) fn connect_function() -> String {
    format!(
        "(name, id, report) => {{
            const page = globalThis[name];
            if (page === undefined || page === null || typeof page.connect !== 'function') {{
                return null;
            }}
            if (page.version !== {PAGE_EXPORT_VERSION}) {{
                throw new Error(`the page's agent speaks version ${{page.version}} of its export, \
                                 not {PAGE_EXPORT_VERSION}`);
            }}
            return page.connect((line) => report(JSON.stringify([id, line])));
        }}"
    )
}

/// Sends its argument on the connection it is called on.
pub(crate) const SEND_FUNCTION: &str = "function (line) { this.send(line); }";

/// Ends the connection it is called on.
pub(crate) const CLOSE_FUNCTION: &str = "function () { this.close(); }";

/// What a report of [`connect_function`]'s says: the connection's ID and the line, or `None`
/// when the app ended the connection. `None` for a report it did not make.
pub(crate) fn read_report(report: &str) -> Option<(u64, Option<String>)> {
    let (id, line) = serde_json::from_str::<(String, Option<String>)>(report).ok()?;
    Some((id.parse::<u64>().ok()?, line))
}

#[cfg(test)]
mod tests;
