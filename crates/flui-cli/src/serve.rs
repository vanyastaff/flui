//! The development web server behind `flui run --device browser:…`.
//!
//! Serves one directory (the web build's output) over plain HTTP on the
//! loopback interface, and reloads the page after each rebuild without a
//! websocket: `index.html` is served with a small script that long-polls
//! [`RELOAD_PATH`], which answers once the server's build generation moves
//! past the one the page saw. [`DevServer::reload`] moves it.
//!
//! It serves the build under its base path (`[web] base_href`, the page's
//! `<base href>`), and answers a navigation to a path that is no file (a
//! deep link into the application's routes) with the page, as history
//! routing needs (ADR-0255 decision 19). Nothing outside the served
//! directory is reachable: the path is percent-decoded and refused when a
//! component leaves the directory, names a drive or a stream, or holds a
//! separator, and the file it names is opened only when its canonical path
//! is still inside the directory, so a symbolic link cannot lead out. A
//! missing asset (a path whose last segment has an extension) and any
//! request that is not a navigation get a 404, never the page. A navigation
//! is a request whose `Sec-Fetch-Mode` is `navigate`; from a browser that
//! sends no `Sec-Fetch-Mode`, one whose `Accept` names HTML. A path names a
//! file only as the file's name is spelled, whatever case the file system
//! ignores. The `Host` check runs on a whole head: one that reaches
//! `MAX_HEAD` without its end is answered 431, one with no `Host` or with
//! two is answered 400, and only a `Host` of this machine's loopback
//! (`localhost`, `127.0.0.1`, `[::1]`) is served, so a page of another site
//! whose name is rebound to 127.0.0.1 gets a 421; these checks come before
//! any path's own answer. At most `MAX_CONNECTIONS` are served at once. One
//! past them is answered 503 on a thread of its own, at most `MAX_REFUSALS`
//! at once and each for at most `REFUSAL`, and closed unanswered past those;
//! a refused request has its bytes read before the close, so the client
//! reads the status. At most `MAX_POLLS` of the connections served may be
//! reload long polls; one past them is a short poll, answered at once with
//! the current generation. The page is served with the generation it
//! is, which its first poll waits past. A request's head must arrive
//! within `REQUEST_HEAD`. Every response is `nosniff`.
//!
//! Standard library only: a thread accepts, a thread per connection serves,
//! and every response closes its connection. A browser fetching a dozen
//! files from a local build does not need more, and the CLI's dependency
//! graph stays where it is.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::build::web_page::BaseHref;

/// The long-poll endpoint the injected script waits on.
const RELOAD_PATH: &str = "/__flui/reload";
/// The injected script itself.
const RELOAD_SCRIPT_PATH: &str = "/__flui/reload.js";
/// How long one long-poll waits before answering with the unchanged
/// generation, so a proxy or the browser never sees a dead request.
const LONG_POLL: Duration = Duration::from_secs(25);
/// Upper bound on a request head; anything longer is not a browser.
const MAX_HEAD: usize = 16 * 1024;

/// The page's script waits for a generation other than the one the page was
/// served with (its `src` carries it, `?generation=N`), so a rebuild between
/// serving the page and its first poll reloads it too. A poll answered at
/// once because too many wait (`short`) is asked again a second later.
const RELOAD_SCRIPT: &str = r#"(async () => {
  const seen = new URL(document.currentScript.src).searchParams.get("generation");
  const pause = () => new Promise((resolve) => setTimeout(resolve, 1000));
  for (;;) {
    try {
      const response = await fetch(`/__flui/reload?since=${seen}`, { cache: "no-store" });
      const { generation, short } = await response.json();
      if (String(generation) !== seen) { location.reload(); return; }
      if (short) await pause();
    } catch (_) {
      await pause();
    }
  }
})();
"#;

/// A running server. Dropping it stops the accept loop.
pub(crate) struct DevServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

struct Shared {
    root: PathBuf,
    /// The path the build is served under.
    base: BaseHref,
    generation: AtomicU64,
    changed: Condvar,
    lock: Mutex<()>,
    stop: AtomicBool,
    /// The connections being served now, at most `MAX_CONNECTIONS`.
    connections: AtomicUsize,
    /// The connections past them being refused now, at most `MAX_REFUSALS`.
    refusals: AtomicUsize,
    /// The reload long polls being held now, at most `MAX_POLLS`.
    polls: AtomicUsize,
}

impl DevServer {
    /// Serve `root` under `base` on `127.0.0.1:port`; `0` picks a free port.
    pub(crate) fn start(root: PathBuf, port: u16, base: BaseHref) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            root,
            base,
            generation: AtomicU64::new(1),
            changed: Condvar::new(),
            lock: Mutex::new(()),
            stop: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            refusals: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
        });
        let accept = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("flui-dev-server".into())
                .spawn(move || accept_loop(&listener, &shared))?
        };
        Ok(Self {
            addr,
            shared,
            accept: Some(accept),
        })
    }

    /// The address to open in a browser: the build's base path.
    pub(crate) fn url(&self) -> String {
        format!("http://{}{}", self.addr, self.shared.base)
    }

    /// The directory being served.
    pub(crate) fn root(&self) -> &Path {
        &self.shared.root
    }

    /// Tell every open page to reload.
    pub(crate) fn reload(&self) {
        self.shared.bump();
    }
}

impl Drop for DevServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so it observes the flag; a failed connect
        // means the listener is already gone, which is the same outcome.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        self.shared.bump();
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

impl Shared {
    fn bump(&self) {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_all();
    }

    /// Wait until the generation differs from `since` or `LONG_POLL` passes.
    fn wait_past(&self, since: Option<u64>) -> u64 {
        let mut guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let deadline = std::time::Instant::now() + LONG_POLL;
        loop {
            let now = self.generation.load(Ordering::SeqCst);
            if since != Some(now) || self.stop.load(Ordering::SeqCst) {
                return now;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return now;
            }
            let (next, _) = self
                .changed
                .wait_timeout(guard, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard = next;
        }
    }
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        // Every connection the server holds is counted until it closes, on a
        // thread of its own, so this loop only accepts. Past `MAX_CONNECTIONS`
        // a connection is refused with a 503 on its own thread, at most
        // `MAX_REFUSALS` at once; past those it is closed at once, so a client
        // that opens connections without end gets no thread for each.
        let (held, work): (Held, fn(TcpStream, &Shared)) =
            if let Some(held) = Held::take(shared, Count::Connections) {
                (held, |stream, shared| {
                    let _ = serve_connection(stream, shared);
                })
            } else if let Some(held) = Held::take(shared, Count::Refusals) {
                (held, |mut stream, _| {
                    let busy = Response::text("503 Service Unavailable", "too many connections\n");
                    if stream.set_write_timeout(Some(REFUSAL)).is_ok() {
                        refuse(&mut stream, &busy, Instant::now() + REFUSAL);
                    }
                })
            } else {
                continue;
            };
        // A thread that cannot start drops `held` with its closure.
        let _ = thread::Builder::new()
            .name("flui-dev-server-conn".into())
            .spawn(move || work(stream, &held.shared));
    }
}

/// What a [`Held`] place counts.
#[derive(Clone, Copy)]
enum Count {
    /// A connection being served, at most `MAX_CONNECTIONS`.
    Connections,
    /// A connection past them being refused, at most `MAX_REFUSALS`.
    Refusals,
}

impl Count {
    fn of(self, shared: &Shared) -> &AtomicUsize {
        match self {
            Self::Connections => &shared.connections,
            Self::Refusals => &shared.refusals,
        }
    }

    fn max(self) -> usize {
        match self {
            Self::Connections => MAX_CONNECTIONS,
            Self::Refusals => MAX_REFUSALS,
        }
    }
}

/// A connection's place, given back however its thread ends, a panic
/// included.
struct Held {
    shared: Arc<Shared>,
    count: Count,
}

impl Held {
    /// A place of `count`, or `None` when all are taken.
    fn take(shared: &Arc<Shared>, count: Count) -> Option<Self> {
        if count.of(shared).fetch_add(1, Ordering::SeqCst) >= count.max() {
            count.of(shared).fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self {
            shared: Arc::clone(shared),
            count,
        })
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.count.of(&self.shared).fetch_sub(1, Ordering::SeqCst);
    }
}

/// One HTTP/1.x exchange, then the connection closes.
fn serve_connection(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request = match read_request(&mut stream)? {
        Head::Request(request) => request,
        Head::Closed => return Ok(()),
        Head::TooLarge => {
            let refused = Response::text("431 Request Header Fields Too Large", "too large\n");
            refuse(&mut stream, &refused, Instant::now() + REQUEST_HEAD);
            return Ok(());
        }
        Head::NoHost => {
            let refused = Response::text("400 Bad Request", "exactly one Host header\n");
            refuse(&mut stream, &refused, Instant::now() + REQUEST_HEAD);
            return Ok(());
        }
    };
    // The method and the `Host` are checked before any path's own answer,
    // the reload poll's included.
    if let Some(refused) = refusal(&request) {
        return write_response(&mut stream, &refused, request.method == "HEAD");
    }
    // A long poll holds its connection for up to `LONG_POLL`: past its own
    // smaller bound it is answered at once with the current generation (a
    // short poll, which the page's script asks again a second later), so
    // polls never take the slots that requests need and every tab still
    // learns of a rebuild.
    let _poll = if request.path == RELOAD_PATH {
        let Some(slot) = Slot::take(&shared.polls, MAX_POLLS) else {
            let generation = shared.generation.load(Ordering::SeqCst);
            let short = Response {
                status: "200 OK",
                content_type: "application/json",
                body: format!("{{\"generation\":{generation},\"short\":true}}").into_bytes(),
            };
            return write_response(&mut stream, &short, false);
        };
        Some(slot)
    } else {
        None
    };
    let response = respond(&request, shared);
    if request.path != RELOAD_PATH {
        crate::ui::debug(format!(
            "{} {} {} ({} bytes)",
            request.method,
            request.path,
            response.status,
            response.body.len()
        ));
    }
    write_response(&mut stream, &response, request.method == "HEAD")
}

struct Request {
    method: String,
    /// Path without the query string.
    path: String,
    query: String,
    /// The browser is navigating to the path, not fetching: its
    /// `Sec-Fetch-Mode` is `navigate`, or, from a browser that sends no
    /// `Sec-Fetch-Mode`, its `Accept` names HTML.
    navigation: bool,
    /// The request's one `Host` header.
    host: String,
}

struct Response {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn text(status: &'static str, body: &str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }
}

/// Parse the request line and the headers that tell a navigation from an
/// asset fetch. `None` for an empty read.
fn read_request(stream: &mut TcpStream) -> io::Result<Head> {
    let deadline = Deadline {
        stream: stream.try_clone()?,
        until: Instant::now() + REQUEST_HEAD,
    };
    let mut reader = BufReader::new(deadline).take(MAX_HEAD as u64);
    // A line that ends without its line feed ends the head unfinished: cut
    // at the cap (`limit() == 0`) it is too large, at the stream's end it is
    // a client that went away.
    let unfinished = |reader: &io::Take<BufReader<Deadline>>| {
        if reader.limit() == 0 {
            Head::TooLarge
        } else {
            Head::Closed
        }
    };
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Ok(unfinished(&reader));
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(Head::Closed);
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (mut fetch_mode, mut accepts_html) = (None, false);
    let mut hosts = Vec::new();
    let mut header = String::new();
    loop {
        header.clear();
        reader.read_line(&mut header)?;
        if !header.ends_with('\n') {
            return Ok(unfinished(&reader));
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("sec-fetch-mode") {
                fetch_mode = Some(value.eq_ignore_ascii_case("navigate"));
            } else if name.eq_ignore_ascii_case("accept") {
                accepts_html = value
                    .split(',')
                    .any(|kind| kind.trim().starts_with("text/html"));
            } else if name.eq_ignore_ascii_case("host") {
                hosts.push(value.to_owned());
            }
        }
    }
    // The `Host` check needs exactly one `Host`: none (HTTP/1.0) or two
    // leave the name the request is for unknown.
    let Ok([host]) = <[String; 1]>::try_from(hosts) else {
        return Ok(Head::NoHost);
    };
    Ok(Head::Request(Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        // A browser that sends `Sec-Fetch-Mode` says what the request is; an
        // older one is navigating when it asks for HTML.
        navigation: fetch_mode.unwrap_or(accepts_html),
        host,
    }))
}

/// A request's head as read.
enum Head {
    /// The client went away before a request line.
    Closed,
    /// A whole head with exactly one `Host`.
    Request(Request),
    /// The head reached `MAX_HEAD` without its end: answered 431.
    TooLarge,
    /// No `Host`, or more than one: answered 400.
    NoHost,
}

/// How long a client has to send a request's head, all of it: a client
/// that sends a byte at a time does not hold its connection for longer.
const REQUEST_HEAD: Duration = Duration::from_secs(10);
/// How many connections are served at once; one past them is answered 503.
const MAX_CONNECTIONS: usize = 64;
/// How many connections past them are refused with a 503 at once, each on
/// its own thread for at most `REFUSAL`; one past them is closed unanswered.
const MAX_REFUSALS: usize = 8;
/// How long a refused connection's 503 may take to write and its request's
/// bytes to be drained.
const REFUSAL: Duration = Duration::from_millis(500);
/// How many of those may be reload long polls, each held for up to
/// `LONG_POLL`; one past them is answered at once with the current
/// generation (a short poll).
const MAX_POLLS: usize = 8;

/// One of `count`'s places, given back when dropped.
struct Slot<'a>(&'a AtomicUsize);

impl<'a> Slot<'a> {
    /// A place of `count`, or `None` when `max` are taken.
    fn take(count: &'a AtomicUsize, max: usize) -> Option<Self> {
        if count.fetch_add(1, Ordering::SeqCst) >= max {
            count.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self(count))
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A stream whose reads end at `until`, however the bytes trickle in.
struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the request head took too long",
            ));
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

/// Whether `host` (a `Host` header) names this machine's loopback, where the
/// server listens: a page of another site that rebinds its name to
/// 127.0.0.1 sends its own name, and is refused.
fn is_loopback_host(host: &str) -> bool {
    let name = if let Some(bracketed) = host.strip_prefix('[') {
        bracketed.split(']').next().map(|name| format!("[{name}]"))
    } else {
        host.split(':').next().map(str::to_owned)
    };
    name.is_some_and(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "[::1]"
        )
    })
}

/// The answer to a request the server serves on no path: a method other
/// than `GET` or `HEAD`, or a `Host` that is not this machine's loopback.
fn refusal(request: &Request) -> Option<Response> {
    if request.method != "GET" && request.method != "HEAD" {
        return Some(Response::text(
            "405 Method Not Allowed",
            "GET or HEAD only\n",
        ));
    }
    if !is_loopback_host(&request.host) {
        return Some(Response::text(
            "421 Misdirected Request",
            "this server answers localhost only\n",
        ));
    }
    None
}

/// The answer to a request [`refusal`] lets through.
fn respond(request: &Request, shared: &Shared) -> Response {
    match request.path.as_str() {
        RELOAD_PATH => {
            let since = request
                .query
                .split('&')
                .find_map(|pair| pair.strip_prefix("since="))
                .and_then(|value| value.parse().ok());
            let generation = shared.wait_past(since);
            Response {
                status: "200 OK",
                content_type: "application/json",
                body: format!("{{\"generation\":{generation}}}").into_bytes(),
            }
        }
        RELOAD_SCRIPT_PATH => Response {
            status: "200 OK",
            content_type: "text/javascript",
            body: RELOAD_SCRIPT.as_bytes().to_vec(),
        },
        // The base with its slash left off is the base too.
        path => match path
            .strip_prefix(shared.base.as_str())
            .or_else(|| (format!("{path}/") == shared.base.as_str()).then_some(""))
        {
            Some(within) => serve_file(
                &shared.root,
                within,
                request.navigation,
                shared.generation.load(Ordering::SeqCst),
            ),
            None => not_found(),
        },
    }
}

fn not_found() -> Response {
    Response::text("404 Not Found", "not found\n")
}

/// The file `request_path` (relative to the base) names under `root`; for a
/// navigation to a path that names no file and whose last segment has no
/// extension, the page.
fn serve_file(root: &Path, request_path: &str, navigation: bool, generation: u64) -> Response {
    let Some(relative) = safe_relative_path(request_path) else {
        return not_found();
    };
    // Decided on the request, before `/` becomes `index.html`: a route ends
    // in a segment with no extension, a trailing slash included.
    let names_a_route = request_path
        .rsplit('/')
        .next()
        .is_none_or(|segment| !segment.contains('.'));
    let (relative, body) = match read_inside(root, &relative) {
        Some(body) => (relative, body),
        None if navigation && names_a_route => {
            let page = PathBuf::from("index.html");
            match read_inside(root, &page) {
                Some(body) => (page, body),
                None => return not_found(),
            }
        }
        None => return not_found(),
    };
    let content_type = mime_for(&relative);
    let body = if relative == Path::new("index.html") {
        inject_reload_script(&body, generation)
    } else {
        body
    };
    Response {
        status: "200 OK",
        content_type,
        body,
    }
}

/// The bytes of the file `relative` names under `root`, when it is a file,
/// its canonical path, every link followed, is still under `root`'s, and it
/// ends in `relative` as written: on a file system that ignores case,
/// `/PKG/APP.JS` names no file, so a type is never decided on another
/// spelling of the file's name.
fn read_inside(root: &Path, relative: &Path) -> Option<Vec<u8>> {
    let root = root.canonicalize().ok()?;
    let file = root.join(relative).canonicalize().ok()?;
    (file.starts_with(&root) && file.ends_with(relative) && file.is_file())
        .then(|| std::fs::read(&file).ok())
        .flatten()
}

/// `request_path`, percent-decoded, as a path under the served directory:
/// `/` and a path ending in `/` are its `index.html`. `None` for a path that
/// does not decode to UTF-8, or that has a component that is not a plain
/// name (`..`, `.`, a root, a drive) or that holds a backslash, a colon or a
/// NUL, which a file system reads as a separator, a drive or a stream.
fn safe_relative_path(request_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(request_path)?;
    let trimmed = decoded.trim_start_matches('/');
    let trimmed = if trimmed.is_empty() || trimmed.ends_with('/') {
        format!("{trimmed}index.html")
    } else {
        trimmed.to_owned()
    };
    if trimmed.contains(['\\', ':', '\0']) {
        return None;
    }
    let mut path = PathBuf::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        path.push(segment);
    }
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then_some(path)
}

/// `path` with each `%XX` decoded; `None` for a malformed escape or bytes
/// that are not UTF-8.
fn percent_decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'%' {
            let hex = bytes.get(at + 1..at + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let hex = std::str::from_utf8(hex).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            decoded.push(byte);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("json" | "map") => "application/json",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Add the reload script before `</body>` (or at the end when there is none).
fn inject_reload_script(html: &[u8], generation: u64) -> Vec<u8> {
    let tag = format!("<script src=\"{RELOAD_SCRIPT_PATH}?generation={generation}\"></script>\n");
    let text = String::from_utf8_lossy(html);
    let Some(at) = text.rfind("</body>") else {
        let mut out = html.to_vec();
        out.extend_from_slice(tag.as_bytes());
        return out;
    };
    let mut out = Vec::with_capacity(html.len() + tag.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
}

/// Answer a refused request with `response` and close: the write side is
/// shut after the response, and the request's bytes still arriving are read
/// and dropped, at most `MAX_HEAD` of them and until `until`, so the client
/// reads the status rather than a connection reset.
fn refuse(stream: &mut TcpStream, response: &Response, until: Instant) {
    if write_response(stream, response, false).is_err() {
        return;
    }
    let _ = stream.shutdown(Shutdown::Write);
    let Ok(stream) = stream.try_clone() else {
        return;
    };
    let _ = io::copy(
        &mut Deadline { stream, until }.take(MAX_HEAD as u64),
        &mut io::sink(),
    );
}

fn write_response(stream: &mut TcpStream, response: &Response, head_only: bool) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )?;
    if !head_only {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(addr: SocketAddr, method: &str, target: &str) -> (String, Vec<u8>) {
        request(addr, method, target, "")
    }

    /// What a browser's address bar or a link sends.
    const NAVIGATION: &str =
        "Accept: text/html,application/xhtml+xml,*/*;q=0.8\r\nSec-Fetch-Mode: navigate\r\n";

    /// A browser navigating to `target`, as its address bar or a link does.
    fn navigate(addr: SocketAddr, target: &str) -> (String, Vec<u8>) {
        request(addr, "GET", target, NAVIGATION)
    }

    fn request(addr: SocketAddr, method: &str, target: &str, headers: &str) -> (String, Vec<u8>) {
        request_to(addr, &addr.to_string(), method, target, headers)
    }

    /// A request whose `Host` header names `host`.
    fn request_to(
        addr: SocketAddr,
        host: &str,
        method: &str,
        target: &str,
        headers: &str,
    ) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("response timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .expect("request timeout");
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: {host}\r\n{headers}\r\n"
        )
        .expect("request");
        read_response(&mut stream, method == "HEAD")
            .unwrap_or_else(|error| panic!("response to {method} {target}: {error}"))
    }

    fn read_response(mut reader: impl Read, head_only: bool) -> io::Result<(String, Vec<u8>)> {
        let mut raw = Vec::new();
        if let Err(error) = reader.read_to_end(&mut raw)
            && error.kind() != io::ErrorKind::ConnectionReset
        {
            return Err(error);
        }
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "header terminator"))?;
        let head = std::str::from_utf8(&raw[..split])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let invalid_length = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid response Content-Length",
            )
        };
        let mut content_length = None;
        for line in head.split("\r\n").skip(1) {
            let (name, value) = line.split_once(':').ok_or_else(invalid_length)?;
            if name.trim() != name {
                return Err(invalid_length());
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return Err(invalid_length());
            }
            if name.eq_ignore_ascii_case("content-length") {
                let value = value.trim_matches([' ', '\t']);
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid_length());
                }
                let length = value.parse::<usize>().map_err(|_| invalid_length())?;
                if content_length.replace(length).is_some() {
                    return Err(invalid_length());
                }
            }
        }
        let content_length = content_length.ok_or_else(invalid_length)?;
        let body = &raw[split + 4..];
        let expected = if head_only { 0 } else { content_length };
        if body.len() != expected {
            return Err(io::Error::new(
                if body.len() < expected {
                    io::ErrorKind::UnexpectedEof
                } else {
                    io::ErrorKind::InvalidData
                },
                format!(
                    "response body: expected {expected} bytes, received {}",
                    body.len()
                ),
            ));
        }
        // RFC 9112 §§6.3, 8: an exact Content-Length completes this response;
        // HEAD has no body. A later reset cannot truncate an already complete
        // message, but neither a reset nor clean EOF excuses missing bytes.
        Ok((head.to_owned(), body.to_vec()))
    }

    #[test]
    fn response_reader_requires_complete_framing_before_accepting_a_reset() {
        struct TerminalError(io::ErrorKind);

        impl Read for TerminalError {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(self.0, "terminal read error"))
            }
        }

        fn response(
            raw: &[u8],
            head_only: bool,
            terminal: Option<io::ErrorKind>,
        ) -> io::Result<(String, Vec<u8>)> {
            let bytes = io::Cursor::new(raw);
            match terminal {
                Some(kind) => read_response(bytes.chain(TerminalError(kind)), head_only),
                None => read_response(bytes, head_only),
            }
        }

        let full = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\0asm";
        let head = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n";
        for terminal in [Some(io::ErrorKind::ConnectionReset), None] {
            let (_, body) = response(full, false, terminal).expect("complete GET response");
            assert_eq!(body, b"\0asm");
            let (_, body) = response(head, true, terminal).expect("complete HEAD response");
            assert_eq!(body, [] as [u8; 0]);

            for invalid in [
                head.as_slice(),
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\0as",
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\0asm!",
                b"HTTP/1.1 200 OK\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: nope\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: \r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: 9999999999999999999999999999999999\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: +4\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: 4, 4\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\ncontent-length: 4\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Length : 4\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 4\r\n\r\n\0asm",
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n",
            ] {
                assert!(
                    response(invalid, false, terminal).is_err(),
                    "invalid framing accepted: {invalid:?}; terminal={terminal:?}"
                );
            }
            assert!(response(full, true, terminal).is_err(), "HEAD has a body");
        }
        assert_eq!(
            response(full, false, Some(io::ErrorKind::Other))
                .expect_err("a complete body does not excuse other I/O errors")
                .kind(),
            io::ErrorKind::Other
        );
    }

    fn fixture() -> (tempfile::TempDir, DevServer) {
        fixture_under(BaseHref::root())
    }

    fn fixture_under(base: BaseHref) -> (tempfile::TempDir, DevServer) {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("pkg")).expect("pkg");
        std::fs::write(
            dir.path().join("index.html"),
            "<html><body><canvas></canvas></body></html>",
        )
        .expect("index");
        std::fs::write(dir.path().join("pkg/app.js"), "export default 1;").expect("js");
        std::fs::write(dir.path().join("pkg/app_bg.wasm"), b"\0asm").expect("wasm");
        let server = DevServer::start(dir.path().to_path_buf(), 0, base).expect("server");
        (dir, server)
    }

    fn serves_files_with_their_mime_types_and_no_caching() {
        let (_dir, server) = fixture();
        let (head, body) = get(server.addr, "GET", "/pkg/app_bg.wasm");
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        assert!(head.contains("Content-Type: application/wasm"), "{head}");
        assert!(head.contains("Cache-Control: no-store"), "{head}");
        assert!(head.contains("X-Content-Type-Options: nosniff"), "{head}");
        assert_eq!(body, b"\0asm");
        let (head, _) = get(server.addr, "GET", "/pkg/app.js?v=1");
        assert!(head.contains("Content-Type: text/javascript"), "{head}");
    }

    fn missing_files_traversal_and_directories_are_404_and_head_has_no_body() {
        let (_dir, server) = fixture();
        for target in [
            "/nope.js",
            "/../Cargo.toml",
            "/pkg/../../etc/passwd",
            "/pkg",
        ] {
            let (head, _) = get(server.addr, "GET", target);
            assert!(head.starts_with("HTTP/1.1 404"), "{target}: {head}");
        }
        let (head, body) = get(server.addr, "HEAD", "/pkg/app.js");
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(body, [] as [u8; 0]);
        let (head, _) = get(server.addr, "POST", "/");
        assert!(head.starts_with("HTTP/1.1 405"), "{head}");
    }

    fn is_page(response: &(String, Vec<u8>)) -> bool {
        response.0.starts_with("HTTP/1.1 200")
            && String::from_utf8_lossy(&response.1).contains("<canvas>")
    }

    fn a_navigation_to_a_route_gets_the_page_and_an_asset_never_does() {
        let (_dir, server) = fixture();
        for route in [
            "/settings",
            "/settings/",
            "/settings/profile/",
            "/settings/profile",
            "/a.b/c",
            "/pkg",
        ] {
            assert!(is_page(&navigate(server.addr, route)), "{route}");
        }
        // A browser that sends no `Sec-Fetch-Mode` navigates when it asks for
        // HTML.
        let legacy = request(server.addr, "GET", "/settings", "Accept: text/html\r\n");
        assert!(is_page(&legacy), "{}", legacy.0);
        let (head, _) = navigate(server.addr, "/settings/profile");
        assert!(head.contains("text/html"), "{head}");
        // A fetch that is not a navigation, or a missing asset, is a 404: a
        // `fetch()` that asks for HTML says it is no navigation.
        let fetch = "Accept: text/html\r\nSec-Fetch-Mode: cors\r\n";
        for (target, headers) in [
            ("/settings/profile", ""),
            ("/settings/profile", fetch),
            ("/missing.js", NAVIGATION),
            ("/pkg/missing.wasm", NAVIGATION),
            ("/settings/logo.png", NAVIGATION),
        ] {
            let response = request(server.addr, "GET", target, headers);
            assert!(
                response.0.starts_with("HTTP/1.1 404"),
                "{target} {headers:?}: {}",
                response.0
            );
        }
        let (_, body) = navigate(server.addr, "/pkg/app.js");
        assert_eq!(body, b"export default 1;", "a file is served as itself");
        // Another spelling of a file's name is no file, whatever the file
        // system makes of case.
        for target in ["/PKG/APP.JS", "/INDEX.HTML", "/pkg/App.js"] {
            let response = get(server.addr, "GET", target);
            assert!(
                response.0.starts_with("HTTP/1.1 404"),
                "{target}: {}",
                response.0
            );
        }
    }

    /// The response head to `request`, written whole as it stands.
    fn raw(addr: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a timeout");
        // A server that answers before reading the whole request may reset
        // the rest of the write: the answer is what counts.
        let _ = stream.write_all(request.as_bytes());
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        String::from_utf8_lossy(&answer).into_owned()
    }

    /// The `Host` check runs on a whole head with exactly one `Host`: a
    /// head cut at the cap is 431, none or two `Host`s 400, so a long URL
    /// cannot push `Host: evil.example` past what is read.
    fn the_host_check_runs_on_a_whole_head_with_one_host() {
        let (_dir, server) = fixture();
        let long = format!(
            "GET /index.html?{} HTTP/1.1\r\nHost: evil.example\r\n\r\n",
            "A".repeat(MAX_HEAD + 1000)
        );
        let local = server.addr.to_string();
        for (request, status, case) in [
            (long, "HTTP/1.1 431", "a head past the cap"),
            (
                "GET /pkg/app.js HTTP/1.0\r\n\r\n".to_owned(),
                "HTTP/1.1 400",
                "no Host",
            ),
            (
                format!("GET /pkg/app.js HTTP/1.1\r\nHost: {local}\r\nHost: evil.example\r\n\r\n"),
                "HTTP/1.1 400",
                "two Hosts",
            ),
            (
                format!("GET /pkg/app.js HTTP/1.1\r\nHost: {local}\r\n\r\n"),
                "HTTP/1.1 200",
                "one loopback Host",
            ),
        ] {
            let answer = raw(server.addr, &request);
            assert!(answer.starts_with(status), "{case}: {answer:.80}");
        }
    }

    /// The page is served with the generation it is, so a rebuild between
    /// serving it and its first poll is not taken as seen: a poll since the
    /// page's generation answers the newer one at once.
    fn the_page_carries_the_generation_it_was_served_with() {
        let (_dir, server) = fixture();
        let local = server.addr.to_string();
        let (_, page) = get(server.addr, "GET", "/");
        let page = String::from_utf8_lossy(&page).into_owned();
        assert!(
            page.contains(&format!("{RELOAD_SCRIPT_PATH}?generation=1")),
            "{page}"
        );
        server.reload();
        let poll = format!("GET {RELOAD_PATH}?since=1 HTTP/1.1\r\nHost: {local}\r\n\r\n");
        let answer = raw(server.addr, &poll);
        assert!(answer.ends_with("{\"generation\":2}"), "{answer}");
        let (_, page) = get(server.addr, "GET", "/");
        assert!(
            String::from_utf8_lossy(&page).contains("?generation=2"),
            "a page served after the rebuild carries its generation"
        );
    }

    /// Reload long polls past their own bound are answered at once with the
    /// current generation (a short poll), and a request still finds a slot
    /// while they wait.
    fn long_polls_cannot_take_every_slot() {
        let (_dir, server) = fixture();
        let local = server.addr.to_string();
        let poll = format!("GET {RELOAD_PATH}?since=1 HTTP/1.1\r\nHost: {local}\r\n\r\n");
        let held: Vec<TcpStream> = (0..MAX_POLLS)
            .map(|_| {
                let mut stream = TcpStream::connect(server.addr).expect("connect");
                stream.write_all(poll.as_bytes()).expect("a poll");
                stream
            })
            .collect();
        // The held polls are read and waiting before the next one comes.
        std::thread::sleep(Duration::from_millis(500));
        let started = Instant::now();
        let answer = raw(server.addr, &poll);
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer:.80}");
        assert!(
            answer.ends_with("{\"generation\":1,\"short\":true}"),
            "{answer}"
        );
        assert!(started.elapsed() < LONG_POLL / 5, "answered at once");
        // The method and `Host` checks come before the short poll: a page of
        // another site rebound to 127.0.0.1 or a `POST` learns no generation.
        for (what, request, status) in [
            (
                "another host",
                format!("GET {RELOAD_PATH}?since=1 HTTP/1.1\r\nHost: evil.example\r\n\r\n"),
                "HTTP/1.1 421",
            ),
            (
                "a POST",
                format!(
                    "POST {RELOAD_PATH}?since=1 HTTP/1.1\r\nHost: {local}\r\nContent-Length: 0\r\n\r\n"
                ),
                "HTTP/1.1 405",
            ),
        ] {
            let answer = raw(server.addr, &request);
            assert!(answer.starts_with(status), "{what}: {answer:.80}");
            assert!(!answer.contains("generation"), "{what}: {answer}");
        }
        let (head, _) = get(server.addr, "GET", "/pkg/app.js");
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        drop(held);
    }

    /// A connection past `MAX_CONNECTIONS` reads its 503, and refused
    /// clients that never read hold no thread the server accepts on: once a
    /// place is free, the next request is served while they drain.
    fn a_connection_over_the_cap_reads_its_503_and_delays_no_other() {
        let (_dir, server) = fixture();
        let mut held: Vec<TcpStream> = (0..MAX_CONNECTIONS)
            .map(|_| TcpStream::connect(server.addr).expect("connect"))
            .collect();
        // Each held connection has its thread, waiting for a head.
        std::thread::sleep(Duration::from_millis(500));
        let local = server.addr.to_string();
        let answer = raw(
            server.addr,
            &format!("GET /pkg/app.js HTTP/1.1\r\nHost: {local}\r\n\r\n"),
        );
        assert!(answer.starts_with("HTTP/1.1 503"), "{answer:.80}");
        // Refused clients that send nothing and never read: each one's drain
        // waits out `REFUSAL`, which on the accepting thread would hold every
        // later accept back by that much.
        let silent: Vec<TcpStream> = (0..MAX_REFUSALS)
            .map(|_| TcpStream::connect(server.addr).expect("connect"))
            .collect();
        let started = Instant::now();
        drop(held.pop());
        // The freed place's thread sees the close.
        std::thread::sleep(Duration::from_millis(100));
        let (served, _) = get(server.addr, "GET", "/pkg/app.js");
        assert!(served.starts_with("HTTP/1.1 200"), "{served}");
        assert!(
            started.elapsed() < REFUSAL * 2,
            "served while the refusals drain: {:?}",
            started.elapsed()
        );
        drop(silent);
        drop(held);
    }

    fn a_request_for_another_host_is_refused() {
        let (_dir, server) = fixture();
        for host in [
            "evil.example",
            "evil.example:80",
            "127.0.0.1.evil.example",
            "[::2]",
        ] {
            let (head, _) = request_to(server.addr, host, "GET", "/pkg/app.js", "");
            assert!(head.starts_with("HTTP/1.1 421"), "{host}: {head}");
        }
        for host in [
            "localhost",
            "LOCALHOST:4000",
            "127.0.0.1",
            "127.0.0.1:9",
            "[::1]:9",
        ] {
            let (head, _) = request_to(server.addr, host, "GET", "/pkg/app.js", "");
            assert!(head.starts_with("HTTP/1.1 200"), "{host}: {head}");
        }
    }

    fn a_request_head_that_trickles_is_cut_off_at_its_deadline() {
        let (_dir, server) = fixture();
        let mut stream = TcpStream::connect(server.addr).expect("connect");
        stream
            .set_read_timeout(Some(REQUEST_HEAD + Duration::from_secs(10)))
            .expect("a timeout");
        let started = Instant::now();
        // A byte now and then, never the end of the head.
        let mut cut = false;
        for _ in 0..40 {
            if stream.write_all(b"G").is_err() {
                cut = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            let mut probe = [0_u8; 1];
            stream
                .set_nonblocking(true)
                .expect("non-blocking for the probe");
            let closed = matches!(stream.read(&mut probe), Ok(0));
            stream.set_nonblocking(false).expect("blocking again");
            if closed {
                cut = true;
                break;
            }
        }
        assert!(cut, "the connection outlived its head's deadline");
        assert!(
            started.elapsed() < REQUEST_HEAD + Duration::from_secs(5),
            "cut at the deadline, not per read: {:?}",
            started.elapsed()
        );
    }

    fn encoded_traversal_and_foreign_paths_are_404_even_for_a_navigation() {
        let (_dir, server) = fixture();
        for target in [
            "/%2e%2e/Cargo.toml",
            "/%2E%2E/%2E%2E/etc/passwd",
            "/pkg/%2e%2e/%2e%2e/flui-serve-outside.txt",
            "/..%2fflui-serve-outside.txt",
            "/..%5cflui-serve-outside.txt",
            "/pkg%5c..%5c..%5cflui-serve-outside.txt",
            "/C:%5cWindows%5cwin.ini",
            "/index.html:stream",
            "/pkg//app.js",
            "/%zz",
            "/%ff%fe",
            "/a%00b",
        ] {
            for response in [
                get(server.addr, "GET", target),
                navigate(server.addr, target),
            ] {
                assert!(
                    response.0.starts_with("HTTP/1.1 404"),
                    "{target}: {}",
                    response.0
                );
            }
        }
        let (_, body) = get(server.addr, "GET", "/pkg/app%5Fbg.wasm");
        assert_eq!(body, b"\0asm", "an escaped name is the file it names");
    }

    fn a_link_out_of_the_directory_is_not_followed() {
        let (dir, server) = fixture();
        let outside = tempfile::tempdir().expect("an outside directory");
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "secret").expect("secret");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&secret, dir.path().join("leak.txt"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&secret, dir.path().join("leak.txt"));
        if linked.is_err() {
            // Windows grants symbolic links to administrators and developer
            // mode only; Linux CI runs this case.
            return;
        }
        for response in [
            get(server.addr, "GET", "/leak.txt"),
            navigate(server.addr, "/leak.txt"),
        ] {
            assert!(response.0.starts_with("HTTP/1.1 404"), "{}", response.0);
        }
    }

    fn a_build_served_under_its_base_path_is_reached_only_there() {
        let base = BaseHref::try_new("/apps/demo/").expect("a base path");
        let (_dir, server) = fixture_under(base);
        assert!(server.url().ends_with("/apps/demo/"), "{}", server.url());
        let (head, body) = get(server.addr, "GET", "/apps/demo/pkg/app_bg.wasm");
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(body, b"\0asm");
        for route in ["/apps/demo/", "/apps/demo", "/apps/demo/settings/profile"] {
            assert!(is_page(&navigate(server.addr, route)), "{route}");
        }
        for target in ["/pkg/app_bg.wasm", "/settings", "/apps/demos/x", "/"] {
            let response = navigate(server.addr, target);
            assert!(
                response.0.starts_with("HTTP/1.1 404"),
                "{target}: {}",
                response.0
            );
        }
        let (head, _) = get(server.addr, "GET", RELOAD_SCRIPT_PATH);
        assert!(
            head.starts_with("HTTP/1.1 200"),
            "the reload script stays at the root: {head}"
        );
    }

    #[test]
    fn dev_server_contract() {
        crate::test_cases::run_cases(&[
            (
                "a navigation to a route gets the page and an asset never does",
                a_navigation_to_a_route_gets_the_page_and_an_asset_never_does,
            ),
            (
                "encoded traversal and foreign paths are 404 even for a navigation",
                encoded_traversal_and_foreign_paths_are_404_even_for_a_navigation,
            ),
            (
                "a link out of the directory is not followed",
                a_link_out_of_the_directory_is_not_followed,
            ),
            (
                "a build served under its base path is reached only there",
                a_build_served_under_its_base_path_is_reached_only_there,
            ),
            (
                "the host check runs on a whole head with one host",
                the_host_check_runs_on_a_whole_head_with_one_host,
            ),
            (
                "the page carries the generation it was served with",
                the_page_carries_the_generation_it_was_served_with,
            ),
            (
                "long polls cannot take every slot",
                long_polls_cannot_take_every_slot,
            ),
            (
                "a connection over the cap reads its 503 and delays no other",
                a_connection_over_the_cap_reads_its_503_and_delays_no_other,
            ),
            (
                "a request for another host is refused",
                a_request_for_another_host_is_refused,
            ),
            (
                "a request head that trickles is cut off at its deadline",
                a_request_head_that_trickles_is_cut_off_at_its_deadline,
            ),
            (
                "serves files with their mime types and no caching",
                serves_files_with_their_mime_types_and_no_caching,
            ),
            (
                "missing files traversal and directories are 404 and head has no body",
                missing_files_traversal_and_directories_are_404_and_head_has_no_body,
            ),
        ]);
    }
}
