//! The page link and its relay: the relay against a tab of the test's own, then both dialects
//! against a real browser where one is installed (`FLUI_TEST_CHROMIUM` and `FLUI_TEST_FIREFOX`
//! name one that is not on `PATH`).

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use flui_protocol::endpoint::PolledStream;
use flui_protocol::framing::LineReader;
use flui_protocol::launch::{ENDPOINT_EXTRA, PAGE_LAUNCH_GLOBAL, TOKEN_EXTRA};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;

use super::launch::{LaunchError, LaunchedBrowser};
use super::relay::Relay;
use super::*;
use crate::agent::listener::polled;
use crate::agent::{Endpoint, Session};
use crate::testing::until_ready;

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

#[test]
fn the_pipe_admits_the_user_whoami_names_by_sid() {
    let sid = "S-1-5-21-1004336348-1177238915-682003330-1001";
    for (table, expected) in [
        (
            format!(
                "\r\nUSER INFORMATION\r\n----------------\r\n\r\nUser Name      SID\r\n\
                 ============== =============================================\r\n\
                 desk\\user      {sid}\r\n"
            ),
            Some(sid),
        ),
        (
            format!(
                "\r\nBENUTZERINFORMATIONEN\r\n\r\nBenutzername   SID\r\n\
                 desk\\a b c     {sid}\r\n"
            ),
            Some(sid),
        ),
        ("\r\nUSER INFORMATION\r\n".to_owned(), None),
    ] {
        assert_eq!(
            crate::agent::listener::sid_in_whoami(&table),
            expected,
            "{table}"
        );
    }
}

/// What the test's tab was asked, and what its page does.
#[derive(Default)]
struct Page {
    /// How many connect calls find no export yet.
    absent_for: usize,
    /// How many connect calls go unanswered in time.
    connect_timeouts: usize,
    /// How many abandon calls go unanswered in time.
    abandon_timeouts: usize,
    /// Every line a connection was sent, by connection.
    received: Vec<(String, String)>,
    /// The connections closed from the relay's side.
    closed: Vec<String>,
    /// The connections the relay let the page drop.
    released: Vec<String>,
    /// The connections the relay abandoned, as the page answered each abandon.
    abandoned: Vec<String>,
    /// Connections the page ends as the next line reaches them, reporting `last` and the end
    /// before the line's send throws.
    end_on_send: Vec<String>,
    /// Connections broken in the page: a line sent on one throws, and nothing is reported.
    broken: Vec<String>,
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
        let mut page = page(&self.0);
        if function == ABANDON_FUNCTION {
            if page.abandon_timeouts > 0 {
                page.abandon_timeouts -= 1;
                return Err(LinkError::Timeout);
            }
            page.abandoned.push(args[0].to_owned());
            return Ok(None);
        }
        assert_eq!(function, connect_function());
        assert_eq!(args[0], PAGE_EXPORT);
        if page.connect_timeouts > 0 {
            page.connect_timeouts -= 1;
            return Err(LinkError::Timeout);
        }
        if page.absent_for > 0 {
            page.absent_for -= 1;
            return Ok(None);
        }
        Ok(Some(Remote(args[1].to_owned())))
    }

    fn call_on(&mut self, remote: &Remote, function: &str, args: &[&str]) -> Result<(), LinkError> {
        let mut page = page(&self.0);
        if function == CLOSE_FUNCTION {
            assert_eq!(args, [remote.0.as_str()], "a close names its connection");
            page.closed.push(remote.0.clone());
            return Ok(());
        }
        assert_eq!(function, SEND_FUNCTION);
        if page.broken.contains(&remote.0) {
            return Err(LinkError::Threw("the connection is closed".into()));
        }
        if let Some(at) = page.end_on_send.iter().position(|id| *id == remote.0) {
            page.end_on_send.remove(at);
            page.broken.push(remote.0.clone());
            let last = serde_json::json!([remote.0, "last"]).to_string();
            let end = serde_json::json!([remote.0, null]).to_string();
            page.events.push_back(LinkEvent::Report(last));
            page.events.push_back(LinkEvent::Report(end));
            return Err(LinkError::Threw("the connection is closed".into()));
        }
        page.received.push((remote.0.clone(), args[0].to_owned()));
        let report = serde_json::json!([remote.0, format!("echo:{}", args[0])]).to_string();
        page.events.push_back(LinkEvent::Report(report));
        Ok(())
    }

    fn release(&mut self, remote: Remote) {
        page(&self.0).released.push(remote.0);
    }

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

/// A client of the session's endpoint, reading line by line. A named pipe takes no read
/// timeout, so the stream is polled.
struct Client {
    stream: PolledStream<Stream>,
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
        Self {
            stream: polled(stream).expect("non-blocking"),
            reader: LineReader::new(1 << 20),
        }
    }

    fn send(&mut self, line: &str) {
        let line = format!("{line}\n");
        let mut written = 0;
        while written < line.len() {
            written += until_ready(WAIT, "room for the line", || {
                (&self.stream).write(&line.as_bytes()[written..])
            })
            .expect("the relay reads");
        }
    }

    /// The next line; `None` once the relay closed the connection.
    fn next(&mut self) -> Option<String> {
        let mut chunk = [0_u8; 4096];
        loop {
            if let Some(line) = self.reader.next_line().expect("a line within bounds") {
                return Some(String::from_utf8(line).expect("UTF-8"));
            }
            match until_ready(WAIT, "line or end", || (&self.stream).read(&mut chunk)) {
                Ok(0) | Err(_) => return None,
                Ok(read) => self.reader.push(&chunk[..read]),
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
    assert_eq!(page(&state).released, closed, "which the page may drop");
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
    eventually(&state, |page| page.released.contains(&ended_id));

    page(&state).events.push_back(LinkEvent::Navigated);
    assert_eq!(staying.next(), None, "a reload ends every connection");
    eventually(&state, |page| {
        page.closed.contains(&staying_id) && page.released.contains(&staying_id)
    });
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

/// A client that falls further behind than the relay keeps for it is closed, and so is its page
/// connection, which the page may then drop. Fails if the relay only forgets the client: the
/// connection would keep its place in the app's agent, a driver's seat and held input included,
/// until the page reloads.
#[test]
fn a_client_too_far_behind_is_closed_with_its_page_connection() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut client = Client::connect(&session);
    client.send("hello");
    assert_eq!(client.next().as_deref(), Some("echo:hello"));
    let id = page(&state).received[0].0.clone();
    // One line, with its newline one byte past what the relay keeps for a client.
    let flood = "x".repeat(super::relay::MAX_UNWRITTEN);
    let report = serde_json::json!([id, flood]).to_string();
    drop(flood);
    page(&state).events.push_back(LinkEvent::Report(report));
    assert_eq!(client.next(), None, "the client is closed");
    eventually(&state, |page| page.closed == [id.clone()]);
    eventually(&state, |page| page.released == [id.clone()]);
}

/// A connection the page ended whose client the relay still holds when the page reloads is
/// released and not closed again in the page, and its client still reads what the page sent it
/// before the end. Its last line, its end and the reload come in one batch of the tab's events,
/// so no turn writes the client first. Fails if a reload closes every open connection in the
/// page, the ones the page ended included, or lets go of their clients with lines unwritten.
#[test]
fn a_reload_releases_a_connection_the_page_ended_without_closing_it() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut client = Client::connect(&session);
    client.send("hello");
    assert_eq!(client.next().as_deref(), Some("echo:hello"));
    let id = page(&state).received[0].0.clone();
    {
        let mut page = page(&state);
        let last = serde_json::json!([id, "last"]).to_string();
        let end = serde_json::json!([id, null]).to_string();
        page.events.push_back(LinkEvent::Report(last));
        page.events.push_back(LinkEvent::Report(end));
        page.events.push_back(LinkEvent::Navigated);
    }
    assert_eq!(
        client.next().as_deref(),
        Some("last"),
        "written across the reload"
    );
    assert_eq!(client.next(), None);
    eventually(&state, |page| page.released == [id.clone()]);
    assert!(
        page(&state).closed.is_empty(),
        "a connection the page ended is not closed again"
    );
}

/// A connection the page ended whose client has not read what it was sent yet is released and
/// not closed again in the page when the relay stops. Fails if a stopping relay closes every
/// open connection in the page, the ones the page ended included.
#[test]
fn a_stopping_relay_releases_a_connection_the_page_ended_without_closing_it() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut client = Client::connect(&session);
    client.send("hello");
    assert_eq!(client.next().as_deref(), Some("echo:hello"));
    let id = page(&state).received[0].0.clone();
    {
        // A line past what any host buffers for a client that does not read, then the end, in
        // one batch: by the time the client reads a byte of the line, the page has ended it.
        let mut page = page(&state);
        let backlog = serde_json::json!([id, "x".repeat(8 << 20)]).to_string();
        let end = serde_json::json!([id, null]).to_string();
        page.events.push_back(LinkEvent::Report(backlog));
        page.events.push_back(LinkEvent::Report(end));
    }
    let mut chunk = [0_u8; 4096];
    let read = until_ready(WAIT, "the backlog", || (&client.stream).read(&mut chunk))
        .expect("the client reads");
    assert!(read > 0, "the relay closed the client before the stop");
    drop(relay);
    let seen = page(&state);
    assert_eq!(seen.released, [id], "released as the relay stops");
    assert!(
        seen.closed.is_empty(),
        "a connection the page ended is not closed again: {:?}",
        seen.closed
    );
}

/// A client carried to a page connection the page holds open, its first line answered and
/// read: where the tests below start. Returns the client and its connection's ID.
fn carried(state: &Arc<Mutex<Page>>, session: &Session) -> (Client, String) {
    let mut client = Client::connect(session);
    client.send("hello");
    assert_eq!(client.next().as_deref(), Some("echo:hello"));
    let id = page(state).received[0].0.clone();
    (client, id)
}

/// A connect call the page does not answer in time is abandoned by the connection's ID, again
/// while the page does not answer the abandon, and only then is the client closed. Fails if an
/// unanswered call is taken for one that opened nothing: the page may run it late, and the
/// connection it opens would hold its place in the app's agent with nobody to close it.
#[test]
fn a_connect_the_page_does_not_answer_is_abandoned_by_its_id() {
    let state = Arc::new(Mutex::new(Page {
        connect_timeouts: 1,
        abandon_timeouts: 1,
        ..Page::default()
    }));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let mut client = Client::connect(&session);
    client.send("hello");
    assert_eq!(client.next(), None, "the client is closed");
    let seen = page(&state);
    assert_eq!(seen.abandoned, ["0"], "abandoned once the page answered");
    assert_eq!(
        seen.abandon_timeouts, 0,
        "the unanswered abandon was made again"
    );
    assert!(
        seen.received.is_empty(),
        "no line went: {:?}",
        seen.received
    );
    assert!(seen.closed.is_empty() && seen.released.is_empty());
}

/// A line the page refuses because it ended the connection leaves the connection ended: its
/// client reads what the page sent before the end, and the connection is released, not closed
/// again. Fails if a refused line closes the connection at once: the client loses the page's
/// last lines, and the page is asked to close what it ended.
#[test]
fn a_line_refused_by_a_connection_the_page_ended_leaves_it_ended() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let (mut client, id) = carried(&state, &session);
    page(&state).end_on_send.push(id.clone());
    client.send("again");
    assert_eq!(client.next().as_deref(), Some("last"));
    assert_eq!(client.next(), None);
    eventually(&state, |page| page.released == [id.clone()]);
    assert!(page(&state).closed.is_empty(), "not closed again");
}

/// A line refused on a connection that broke in the page, with no end reported, closes the
/// connection there and releases it once the tab's events show no end. Fails if a refused line
/// waits for an end that never comes.
#[test]
fn a_line_refused_by_a_broken_connection_closes_it() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let (mut client, id) = carried(&state, &session);
    page(&state).broken.push(id.clone());
    client.send("again");
    assert_eq!(client.next(), None);
    eventually(&state, |page| page.released == [id.clone()]);
    assert_eq!(page(&state).closed, [id]);
}

/// A line the page reports after its own end is not the client's: it is dropped, even past what
/// the relay keeps for a client, and the client reads what came before the end. Fails if such a
/// line counts against the bound, which lets go of the client with the page's last line
/// unwritten.
#[test]
fn a_line_reported_after_the_end_is_dropped() {
    let state = Arc::new(Mutex::new(Page::default()));
    let (_project, session, _relay) = relayed(Box::new(Tab(Arc::clone(&state))));
    let (mut client, id) = carried(&state, &session);
    {
        let flood = serde_json::json!([id, "x".repeat(super::relay::MAX_UNWRITTEN)]).to_string();
        let mut page = page(&state);
        let last = serde_json::json!([id, "last"]).to_string();
        let end = serde_json::json!([id, null]).to_string();
        page.events.push_back(LinkEvent::Report(last));
        page.events.push_back(LinkEvent::Report(end));
        page.events.push_back(LinkEvent::Report(flood));
    }
    assert_eq!(client.next().as_deref(), Some("last"));
    assert_eq!(client.next(), None);
    eventually(&state, |page| page.released == [id.clone()]);
    assert!(page(&state).closed.is_empty(), "not closed again");
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
/// How many times a test starts a browser that did not get ready. A cold start on a busy CI
/// machine sometimes does not; each failed attempt is printed with the browser's stderr, and
/// the test fails with the last one.
const START_ATTEMPTS: usize = 2;

fn start_browser(executable: &Path, dialect: Dialect) -> (LaunchedBrowser, Box<dyn BrowserLink>) {
    let mut attempt = 1;
    loop {
        match LaunchedBrowser::start(executable, dialect, true) {
            Ok(started) => return started,
            Err(failure @ LaunchError::NotReady { .. }) if attempt < START_ATTEMPTS => {
                eprintln!("start attempt {attempt} of {START_ATTEMPTS} failed: {failure}");
                attempt += 1;
            }
            Err(failure) => {
                panic!("the browser starts and links (attempt {attempt}): {failure}")
            }
        }
    }
}

fn drives_a_page(executable: &Path, dialect: Dialect) {
    let site = tempfile::tempdir().expect("a site directory");
    std::fs::write(site.path().join("index.html"), APP_PAGE).expect("the page is written");
    let server = crate::serve::DevServer::start(
        site.path().to_path_buf(),
        0,
        crate::build::web_page::BaseHref::root(),
    )
    .expect("the dev server starts");
    let (browser, mut link) = start_browser(executable, dialect);

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

    let profile = browser.profile().to_path_buf();
    drop(browser);
    assert!(!profile.exists(), "the browser's profile outlived it");
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
