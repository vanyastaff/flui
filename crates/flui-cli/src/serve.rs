//! The development web server behind `flui run --device browser:…`.
//!
//! Serves one directory (the web build's output) over plain HTTP on the
//! loopback interface, and reloads the page after each rebuild without a
//! websocket: `index.html` is served with a small script that long-polls
//! [`RELOAD_PATH`], which answers once the server's build generation moves
//! past the one the page saw. [`DevServer::reload`] moves it.
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
    generation: AtomicU64,
    changed: Condvar,
    lock: Mutex<()>,
    stop: AtomicBool,
}

impl DevServer {
    /// Serve `root` on `127.0.0.1:port`; `0` picks a free port.
    pub(crate) fn start(root: PathBuf, port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            root,
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

    /// The address to open in a browser.
    pub(crate) fn url(&self) -> String {
        format!("http://{}/", self.addr)
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

/// Parse the request line and skip the headers. `None` for an empty read.
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
    // Headers carry nothing this server acts on; drain to the blank line.
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" || header == "\n" {
            break;
        }
    }
    Ok(Some(Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
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
        path => serve_file(&shared.root, path),
    }
}

fn serve_file(root: &Path, request_path: &str) -> Response {
    let Some(relative) = safe_relative_path(request_path) else {
        return Response::text("404 Not Found", "not found\n");
    };
    let file = root.join(&relative);
    let Ok(body) = std::fs::read(&file) else {
        return Response::text("404 Not Found", "not found\n");
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

/// `/` is `index.html`; a path that leaves the root (`..`, an absolute
/// component) or names a directory is rejected rather than resolved.
fn safe_relative_path(request_path: &str) -> Option<PathBuf> {
    let trimmed = request_path.trim_start_matches('/');
    let trimmed = if trimmed.is_empty() || trimmed.ends_with('/') {
        format!("{trimmed}index.html")
    } else {
        trimmed.to_string()
    };
    let path = PathBuf::from(&trimmed);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path)
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
        let mut stream = TcpStream::connect(addr).expect("connect");
        write!(stream, "{method} {target} HTTP/1.1\r\nHost: x\r\n\r\n").expect("request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("response");
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("header terminator");
        (
            String::from_utf8_lossy(&raw[..split]).into_owned(),
            raw[split + 4..].to_vec(),
        )
    }

    fn fixture() -> (tempfile::TempDir, DevServer) {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("pkg")).expect("pkg");
        std::fs::write(
            dir.path().join("index.html"),
            "<html><body><canvas></canvas></body></html>",
        )
        .expect("index");
        std::fs::write(dir.path().join("pkg/app.js"), "export default 1;").expect("js");
        std::fs::write(dir.path().join("pkg/app_bg.wasm"), b"\0asm").expect("wasm");
        let server = DevServer::start(dir.path().to_path_buf(), 0).expect("server");
        (dir, server)
    }

    #[test]
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

    #[test]
    fn index_gets_the_reload_script_and_root_means_index() {
        let (_dir, server) = fixture();
        for target in ["/", "/index.html"] {
            let (head, body) = get(server.addr, "GET", target);
            assert!(head.contains("text/html"), "{head}");
            let body = String::from_utf8(body).expect("utf-8");
            assert!(
                body.contains("<script src=\"/__flui/reload.js\"></script>\n</body>"),
                "{body}"
            );
        }
        let (head, body) = get(server.addr, "GET", RELOAD_SCRIPT_PATH);
        assert!(head.contains("text/javascript"), "{head}");
        assert!(String::from_utf8_lossy(&body).contains("location.reload()"));
    }

    #[test]
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
        assert!(body.is_empty());
        let (head, _) = get(server.addr, "POST", "/");
        assert!(head.starts_with("HTTP/1.1 405"), "{head}");
    }

    #[test]
    fn long_poll_answers_when_the_generation_moves() {
        let (_dir, server) = fixture();
        let (_, body) = get(server.addr, "GET", RELOAD_PATH);
        let first: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let seen = first["generation"].as_u64().expect("generation");

        let addr = server.addr;
        let waiter =
            thread::spawn(move || get(addr, "GET", &format!("{RELOAD_PATH}?since={seen}")));
        thread::sleep(Duration::from_millis(100));
        assert!(!waiter.is_finished(), "must block while nothing changed");
        server.reload();
        let (_, body) = waiter.join().expect("waiter");
        let next: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(next["generation"].as_u64(), Some(seen + 1));
    }

    #[test]
    fn dropping_the_server_frees_the_port() {
        let (_dir, server) = fixture();
        let addr = server.addr;
        drop(server);
        assert!(
            TcpListener::bind(addr).is_ok(),
            "port must be released once the server is dropped"
        );
    }

    #[test]
    fn injection_without_a_body_tag_appends() {
        let out = inject_reload_script(b"<canvas></canvas>");
        assert!(
            String::from_utf8(out)
                .expect("utf-8")
                .ends_with("</script>\n")
        );
    }
}
