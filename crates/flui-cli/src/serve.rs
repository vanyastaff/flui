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
//! request that is not a navigation get a 404, never the page.
//!
//! Standard library only: a thread accepts, a thread per connection serves,
//! and every response closes its connection. A browser fetching a dozen
//! files from a local build does not need more, and the CLI's dependency
//! graph stays where it is.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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

const RELOAD_SCRIPT: &str = r#"(async () => {
  let seen = null;
  for (;;) {
    try {
      const response = await fetch(`/__flui/reload?since=${seen ?? ""}`, { cache: "no-store" });
      const { generation } = await response.json();
      if (seen !== null && generation !== seen) { location.reload(); return; }
      seen = generation;
    } catch (_) {
      await new Promise((resolve) => setTimeout(resolve, 1000));
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
        let shared = Arc::clone(shared);
        let _ = thread::Builder::new()
            .name("flui-dev-server-conn".into())
            .spawn(move || {
                let _ = serve_connection(stream, &shared);
            });
    }
}

/// One HTTP/1.x exchange, then the connection closes.
fn serve_connection(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let Some(request) = read_request(&mut stream)? else {
        return Ok(());
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
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    write_response(&mut stream, &response, request.method == "HEAD")
}

struct Request {
    method: String,
    /// Path without the query string.
    path: String,
    query: String,
    /// The browser is navigating to the path (`Sec-Fetch-Mode: navigate`, or
    /// an `Accept` that names HTML), not fetching an asset.
    navigation: bool,
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
fn read_request(stream: &mut TcpStream) -> io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_HEAD as u64);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(None);
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut navigation = false;
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim();
            navigation |= (name.eq_ignore_ascii_case("sec-fetch-mode")
                && value.eq_ignore_ascii_case("navigate"))
                || (name.eq_ignore_ascii_case("accept")
                    && value
                        .split(',')
                        .any(|kind| kind.trim().starts_with("text/html")));
        }
    }
    Ok(Some(Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        navigation,
    }))
}

fn respond(request: &Request, shared: &Shared) -> Response {
    if request.method != "GET" && request.method != "HEAD" {
        return Response::text("405 Method Not Allowed", "GET or HEAD only\n");
    }
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
            Some(within) => serve_file(&shared.root, within, request.navigation),
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
fn serve_file(root: &Path, request_path: &str, navigation: bool) -> Response {
    let Some(relative) = safe_relative_path(request_path) else {
        return not_found();
    };
    let (relative, body) = match read_inside(root, &relative) {
        Some(body) => (relative, body),
        None if navigation && relative.extension().is_none() => {
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
        inject_reload_script(&body)
    } else {
        body
    };
    Response {
        status: "200 OK",
        content_type,
        body,
    }
}

/// The bytes of the file `relative` names under `root`, when it is a file
/// and its canonical path, every link followed, is still under `root`'s.
fn read_inside(root: &Path, relative: &Path) -> Option<Vec<u8>> {
    let root = root.canonicalize().ok()?;
    let file = root.join(relative).canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file())
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
fn inject_reload_script(html: &[u8]) -> Vec<u8> {
    let tag = format!("<script src=\"{RELOAD_SCRIPT_PATH}\"></script>\n");
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

fn write_response(stream: &mut TcpStream, response: &Response, head_only: bool) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
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

    /// A browser navigating to `target`, as its address bar or a link does.
    fn navigate(addr: SocketAddr, target: &str) -> (String, Vec<u8>) {
        request(
            addr,
            "GET",
            target,
            "Accept: text/html,application/xhtml+xml,*/*;q=0.8\r\nSec-Fetch-Mode: navigate\r\n",
        )
    }

    fn request(addr: SocketAddr, method: &str, target: &str, headers: &str) -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("response timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .expect("request timeout");
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: x\r\n{headers}\r\n"
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
        for route in ["/settings", "/settings/profile", "/a.b/c", "/pkg"] {
            assert!(is_page(&navigate(server.addr, route)), "{route}");
        }
        let (head, _) = navigate(server.addr, "/settings/profile");
        assert!(head.contains("text/html"), "{head}");
        // A fetch that is not a navigation, or a missing asset, is a 404.
        for (target, navigation) in [
            ("/settings/profile", false),
            ("/missing.js", true),
            ("/pkg/missing.wasm", true),
            ("/settings/logo.png", true),
        ] {
            let response = if navigation {
                navigate(server.addr, target)
            } else {
                get(server.addr, "GET", target)
            };
            assert!(
                response.0.starts_with("HTTP/1.1 404"),
                "{target}: {}",
                response.0
            );
        }
        let (_, body) = navigate(server.addr, "/pkg/app.js");
        assert_eq!(body, b"export default 1;", "a file is served as itself");
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
