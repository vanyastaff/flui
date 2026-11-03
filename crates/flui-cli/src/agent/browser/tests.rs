//! The page link and its relay: the relay against a tab of the test's own, then both dialects
//! against a real browser where one is installed (`FLUI_TEST_CHROMIUM` and `FLUI_TEST_FIREFOX`
//! name one that is not on `PATH`).

use std::collections::VecDeque;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use flui_protocol::framing::LineReader;
use flui_protocol::launch::{ENDPOINT_EXTRA, PAGE_LAUNCH_GLOBAL, TOKEN_EXTRA};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;

use super::launch::LaunchedBrowser;
use super::relay::Relay;
use super::*;
use crate::agent::{Endpoint, Session};

/// How long a test waits for a line or an end.
const WAIT: Duration = Duration::from_secs(20);

#[test]
fn a_browser_is_reached_in_the_protocol_it_has_without_a_driver() {
    for (id, dialect) in [
        ("browser:firefox", Some(Dialect::Bidi)),
        ("browser:chrome", Some(Dialect::Cdp)),
        ("browser:chromium", Some(Dialect::Cdp)),
        ("browser:edge", Some(Dialect::Cdp)),
        ("browser:brave", Some(Dialect::Cdp)),
        ("browser:safari", None),
        ("browser:internet explorer", None),
        ("firefox", None),
    ] {
        assert_eq!(Dialect::of(id), dialect, "{id}");
    }
}

#[test]
fn the_page_scripts_install_the_launch_and_report_by_connection() {
    let preload = preload(&[(ENDPOINT_EXTRA, "__flui_agent"), (TOKEN_EXTRA, "a\"b")]);
    assert!(
        preload.contains(&format!("globalThis[\"{PAGE_LAUNCH_GLOBAL}\"]")),
        "{preload}"
    );
    assert!(
        preload.contains(r#""flui.agent.token":"a\"b""#),
        "values are JSON: {preload}"
    );
    assert!(
        preload.contains("globalThis.parent !== globalThis"),
        "a frame is given no launch: {preload}"
    );
    assert!(connect_function().contains("page.version !== 1"));

    assert_eq!(
        read_report(r#"["7","{\"id\":1}"]"#),
        Some((7, Some("{\"id\":1}".to_owned())))
    );
    assert_eq!(read_report(r#"["7",null]"#), Some((7, None)));
    for foreign in [r#"[7,"line"]"#, r#"["x","line"]"#, "line", r#"["7"]"#] {
        assert_eq!(read_report(foreign), None, "{foreign}");
    }
}

/// What the test's tab was asked, and what its page does.
#[derive(Default)]
struct Page {
    /// How many connect calls find no export yet.
    absent_for: usize,
    /// Every line a connection was sent, by connection.
    received: Vec<(String, String)>,
    /// The connections closed from the relay's side.
    closed: Vec<String>,
    /// Events the tab reports next.
    events: VecDeque<LinkEvent>,
    /// Set: the browser went.
    gone: bool,
}

/// A tab of the test's own: its page defines an export that answers each line with
/// `echo:<line>`.
struct Tab(Arc<Mutex<Page>>);

fn page(page: &Mutex<Page>) -> MutexGuard<'_, Page> {
    page.lock().expect("BUG: the tab's page is not poisoned")
}

impl BrowserLink for Tab {
    fn open(&mut self, _preload: &str, _url: &str) -> Result<(), LinkError> {
        Ok(())
    }

    fn call(&mut self, function: &str, args: &[&str]) -> Result<Option<Remote>, LinkError> {
        assert_eq!(function, connect_function());
        assert_eq!(args[0], PAGE_EXPORT);
        let mut page = page(&self.0);
        if page.absent_for > 0 {
            page.absent_for -= 1;
            return Ok(None);
        }
        Ok(Some(Remote(args[1].to_owned())))
    }

    fn call_on(&mut self, remote: &Remote, function: &str, args: &[&str]) -> Result<(), LinkError> {
        let mut page = page(&self.0);
        if function == CLOSE_FUNCTION {
            page.closed.push(remote.0.clone());
            return Ok(());
        }
        assert_eq!(function, SEND_FUNCTION);
        page.received.push((remote.0.clone(), args[0].to_owned()));
        let report = serde_json::json!([remote.0, format!("echo:{}", args[0])]).to_string();
        page.events.push_back(LinkEvent::Report(report));
        Ok(())
    }

    fn release(&mut self, _remote: Remote) {}

    fn next_event(&mut self, deadline: Instant) -> Result<Option<LinkEvent>, LinkError> {
        let mut page = page(&self.0);
        if page.gone {
            return Err(LinkError::Gone("the test closed the browser".into()));
        }
        if let Some(event) = page.events.pop_front() {
            return Ok(Some(event));
        }
        drop(page);
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        Ok(None)
    }
}

/// A session's endpoint and the relay serving it for `link`.
fn relayed(link: Box<dyn BrowserLink>) -> (tempfile::TempDir, Session, Relay) {
    let project = tempfile::tempdir().expect("a project directory");
    let session = Session::create_relayed(project.path(), None, Some(project.path()), PAGE_EXPORT)
        .expect("a relayed session");
    let Endpoint::Local(endpoint) = &session.credentials().endpoint else {
        panic!("a relayed session serves a local endpoint");
    };
    let relay = Relay::start(link, endpoint, PAGE_EXPORT.to_owned()).expect("the relay starts");
    (project, session, relay)
}

/// A client of the session's endpoint, reading line by line.
struct Client {
    stream: Stream,
    reader: LineReader,
}

impl Client {
    fn connect(session: &Session) -> Self {
        let Endpoint::Local(endpoint) = &session.credentials().endpoint else {
            panic!("a relayed session serves a local endpoint");
        };
        let stream =
            Stream::connect(crate::agent::client::name(endpoint).expect("the endpoint is a name"))
                .expect("the relay accepts");
        stream
            .set_recv_timeout(Some(Duration::from_millis(100)))
            .expect("a receive timeout");
        Self {
            stream,
            reader: LineReader::new(1 << 20),
        }
    }

    fn send(&mut self, line: &str) {
        (&self.stream)
            .write_all(format!("{line}\n").as_bytes())
            .expect("the relay reads");
    }

    /// The next line; `None` once the relay closed the connection.
    fn next(&mut self) -> Option<String> {
        let deadline = Instant::now() + WAIT;
        let mut chunk = [0_u8; 4096];
        loop {
            if let Some(line) = self.reader.next_line().expect("a line within bounds") {
                return Some(String::from_utf8(line).expect("UTF-8"));
            }
            assert!(
                Instant::now() < deadline,
                "no line and no end within {WAIT:?}"
            );
            match (&self.stream).read(&mut chunk) {
                Ok(0) => return None,
                Ok(read) => self.reader.push(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return None,
            }
        }
    }
}

/// Waits until `done` holds of the page.
fn eventually(page: &Mutex<Page>, done: impl Fn(&Page) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done(&self::page(page)) {
        assert!(Instant::now() < deadline, "the page never got there");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_client_waits_for_the_page_export_and_is_carried_to_a_connection_of_its_own() {
    let state = Arc::new(Mutex::new(Page {
        absent_for: 3,
        ..Page::default()
    }));
    let (_project, session, relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut first = Client::connect(&session);
    first.send("hello");
    first.send("again");
    // The page the relay opened loads: a client waiting for its export waits on.
    page(&state).events.push_back(LinkEvent::Navigated);
    assert_eq!(
        first.next().as_deref(),
        Some("echo:hello"),
        "sent before the export"
    );
    assert_eq!(first.next().as_deref(), Some("echo:again"), "in order");
    let mut second = Client::connect(&session);
    second.send("other");
    assert_eq!(second.next().as_deref(), Some("echo:other"));
    let received = page(&state).received.clone();
    assert_eq!(received[0].1, "hello");
    assert_ne!(
        received[0].0, received[2].0,
        "each client has a connection of its own"
    );
    drop(relay);
    assert_eq!(first.next(), None, "a stopped relay closes its clients");
    let closed = page(&state).closed.clone();
    assert_eq!(closed.len(), 2, "and their page connections: {closed:?}");
}

#[test]
fn a_page_connection_ends_with_its_client_and_every_one_with_a_reload() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut leaving = Client::connect(&session);
    leaving.send("hello");
    assert_eq!(leaving.next().as_deref(), Some("echo:hello"));
    let leaving_id = page(&state).received[0].0.clone();
    drop(leaving);
    eventually(&state, |page| page.closed == [leaving_id.clone()]);

    let mut staying = Client::connect(&session);
    staying.send("hello");
    assert_eq!(staying.next().as_deref(), Some("echo:hello"));
    let staying_id = page(&state).received[1].0.clone();
    // The page ends one connection: its client reads what it was sent, then the end.
    let mut ended = Client::connect(&session);
    ended.send("hello");
    assert_eq!(ended.next().as_deref(), Some("echo:hello"));
    let ended_id = page(&state).received[2].0.clone();
    {
        let mut page = page(&state);
        let last = serde_json::json!([ended_id, "last"]).to_string();
        let end = serde_json::json!([ended_id, null]).to_string();
        page.events.push_back(LinkEvent::Report(last));
        page.events.push_back(LinkEvent::Report(end));
    }
    assert_eq!(ended.next().as_deref(), Some("last"));
    assert_eq!(ended.next(), None);

    page(&state).events.push_back(LinkEvent::Navigated);
    assert_eq!(staying.next(), None, "a reload ends every connection");
    eventually(&state, |page| page.closed.contains(&staying_id));
    assert!(
        !page(&state).closed.contains(&ended_id),
        "a connection the page ended is not closed again"
    );
    let mut after = Client::connect(&session);
    after.send("again");
    assert_eq!(
        after.next().as_deref(),
        Some("echo:again"),
        "and the next connects"
    );
}

#[test]
fn the_relay_ends_with_its_browser() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut client = Client::connect(&session);
    client.send("hello");
    assert_eq!(client.next().as_deref(), Some("echo:hello"));
    page(&state).gone = true;
    assert_eq!(client.next(), None);
    let deadline = Instant::now() + WAIT;
    while relay.is_serving() {
        assert!(Instant::now() < deadline, "the relay outlived its browser");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A page standing in for a FLUI app: it defines the page export a moment after it loads,
/// and each connection answers a line with the line and the token the launch gave the page.
const APP_PAGE: &str = r#"<!doctype html>
<html><body><script>
  const launch = globalThis.__FLUI_LAUNCH__;
  delete globalThis.__FLUI_LAUNCH__;
  setTimeout(() => {
    globalThis[launch["flui.agent.endpoint"]] = {
      version: 1,
      connect(onLine) {
        return {
          version: 1,
          send(line) {
            if (line === "end") { onLine(null); return; }
            onLine(`${line}:${launch["flui.agent.token"]}`);
          },
          close() {},
        };
      },
    };
  }, 300);
</script></body></html>
"#;

/// The browser `variable` names, or the first of `commands` on `PATH`.
fn installed_browser(variable: &str, commands: &[&str]) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(variable).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    commands
        .iter()
        .find_map(|command| which::which(command).ok())
}

/// The whole path in a real browser: the launch object installed before the page's scripts,
/// a client carried to the page's export, and a reload ending the connection.
fn drives_a_page(executable: &Path, dialect: Dialect) {
    let site = tempfile::tempdir().expect("a site directory");
    std::fs::write(site.path().join("index.html"), APP_PAGE).expect("the page is written");
    let server = crate::serve::DevServer::start(site.path().to_path_buf(), 0)
        .expect("the dev server starts");
    let (_browser, mut link) =
        LaunchedBrowser::start(executable, dialect, true).expect("the browser starts and links");

    let project = tempfile::tempdir().expect("a project directory");
    let session = Session::create_relayed(project.path(), None, Some(project.path()), PAGE_EXPORT)
        .expect("a relayed session");
    link.open(&preload(&session.page_launch()), &server.url())
        .expect("the page opens");
    let Endpoint::Local(endpoint) = &session.credentials().endpoint else {
        panic!("a relayed session serves a local endpoint");
    };
    let _relay = Relay::start(link, endpoint, PAGE_EXPORT.to_owned()).expect("the relay starts");
    let token = session.credentials().token.clone();

    let mut client = Client::connect(&session);
    client.send("hello");
    assert_eq!(client.next(), Some(format!("hello:{token}")));
    client.send("end");
    assert_eq!(client.next(), None, "the page ended the connection");

    let mut reloaded = Client::connect(&session);
    reloaded.send("before");
    assert_eq!(reloaded.next(), Some(format!("before:{token}")));
    server.reload();
    assert_eq!(reloaded.next(), None, "the reload ended the connection");
    let mut after = Client::connect(&session);
    after.send("after");
    assert_eq!(
        after.next(),
        Some(format!("after:{token}")),
        "the reloaded page was given its launch again"
    );
}

#[test]
fn chromium_is_driven_over_the_devtools_protocol() {
    let Some(chromium) = installed_browser(
        "FLUI_TEST_CHROMIUM",
        &["google-chrome", "chromium", "chromium-browser"],
    ) else {
        eprintln!("skipped: no Chromium on PATH or in FLUI_TEST_CHROMIUM");
        return;
    };
    drives_a_page(&chromium, Dialect::Cdp);
}

#[test]
fn firefox_is_driven_over_webdriver_bidi() {
    let Some(firefox) = installed_browser("FLUI_TEST_FIREFOX", &["firefox"]) else {
        eprintln!("skipped: no Firefox on PATH or in FLUI_TEST_FIREFOX");
        return;
    };
    drives_a_page(&firefox, Dialect::Bidi);
}
