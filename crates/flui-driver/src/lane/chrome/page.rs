//! The web build's files served over HTTP on the loopback, for as long as the session runs.
//!
//! A page needs an origin a browser loads a module and its wasm from, and a file URL is none.
//! The server answers `GET` and `HEAD` of the files under its root and nothing else, on
//! `127.0.0.1` alone:
//!
//! - **Bounded.** A request's head is read up to [`MAX_HEAD`] bytes within [`READ_TIMEOUT`], and
//!   the whole connection, its answer written included, within [`CONNECTION_DEADLINE`], however
//!   slowly the client reads; one connection is served at a time and each is closed after its
//!   answer, so the browser's parallel requests queue in the listener's backlog rather than in
//!   threads, and dropping the server returns once the connection in hand reaches its deadline.
//! - **For its own address alone.** A request whose `Host` is not the address and port the server
//!   bound is refused (`421`), so a page of another origin that rebinds its name to the loopback
//!   reads nothing.
//! - **Inside its root.** A path is a sequence of plain names: no `..`, `.`, empty segment,
//!   backslash, drive, percent escape or control character is resolved, and the file found is
//!   checked to lie under the root once links are followed. Anything else is `404`.

use std::fs::File;
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// The most bytes of a request's head the server reads.
pub const MAX_HEAD: usize = 8 << 10;

/// How long a connection may take to send its request's head.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one connection may last, its answer written included.
pub const CONNECTION_DEADLINE: Duration = Duration::from_secs(10);

/// How long a refused request's connection is read for what its client still sends, before it
/// closes.
const LINGER: Duration = Duration::from_millis(200);

/// A directory served on the loopback, for its own `Host` alone, each connection within a
/// deadline. Serving stops when it drops.
#[derive(Debug)]
pub struct PageServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PageServer {
    /// Serve the files under `root` on a free port of `127.0.0.1`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when `root` is not a directory; when no port can be bound.
    pub fn serve(root: &Path) -> Result<Self> {
        let root = root.canonicalize().map_err(|error| {
            Error::invalid_argument(format!(
                "the web build at {} cannot be served: {error}",
                root.display()
            ))
        })?;
        if !root.is_dir() {
            return Err(Error::invalid_argument(format!(
                "the web build at {} is not a directory",
                root.display()
            )));
        }
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let host = address.to_string();
        let thread = thread::Builder::new()
            .name("flui-driver-page".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::Acquire) {
                        return;
                    }
                    if let Ok(stream) = stream {
                        let _ = answer(&root, &host, stream);
                    }
                }
            })?;
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }

    /// The page's address, ending in `/`.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/", self.address)
    }
}

impl Drop for PageServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the accept, which then sees the stop; the connection in hand ends by its deadline.
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The time left before a deadline, as a socket timeout: an error once it has passed.
fn left(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::Error::from(io::ErrorKind::TimedOut));
    }
    Ok(left)
}

/// Answer one connection's request, all of it before [`CONNECTION_DEADLINE`].
fn answer(root: &Path, host: &str, stream: TcpStream) -> io::Result<()> {
    let opened = Instant::now();
    let deadline = opened + CONNECTION_DEADLINE;
    // The whole head within its own deadline, however slowly it trickles in.
    let head_deadline = opened + READ_TIMEOUT;
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_HEAD as u64);
    let mut request_line = String::new();
    stream.set_read_timeout(Some(left(head_deadline)?))?;
    reader.read_line(&mut request_line)?;
    // The rest of the head, to its blank line, within the bound.
    let mut complete = false;
    let mut named_host: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        stream.set_read_timeout(Some(left(head_deadline)?))?;
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line == "\r\n" || line == "\n" {
            complete = true;
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("host")
        {
            named_host = Some(value.trim().to_owned());
        }
    }
    let mut stream = stream;
    let refuse =
        |stream: &mut TcpStream, status: &str| respond(stream, status, None, false, deadline);
    if !complete {
        return refuse(&mut stream, "431 Request Header Fields Too Large");
    }
    if named_host.as_deref() != Some(host) {
        return refuse(&mut stream, "421 Misdirected Request");
    }
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return refuse(&mut stream, "400 Bad Request");
    };
    let head = match method {
        "GET" => false,
        "HEAD" => true,
        _ => return refuse(&mut stream, "405 Method Not Allowed"),
    };
    match resolve(root, target) {
        Some(file) => respond(&mut stream, "200 OK", Some(&file), head, deadline),
        None => refuse(&mut stream, "404 Not Found"),
    }
}

/// The file under `root` the request target `target` names, when it names one there.
#[must_use]
pub fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next()?;
    let relative = path.strip_prefix('/')?;
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let mut file = root.to_path_buf();
    for segment in relative.split('/') {
        let plain = !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment
                .chars()
                .any(|c| c == '\\' || c == ':' || c == '%' || c.is_control());
        if !plain {
            return None;
        }
        file.push(segment);
    }
    let found = file.canonicalize().ok()?;
    (found.starts_with(root) && found.is_file()).then_some(found)
}

/// The media type a browser takes the file at `path` as.
fn media_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Write `bytes` whole before `deadline`, each write given only the time left.
fn write_by(stream: &mut TcpStream, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        stream.set_write_timeout(Some(left(deadline)?))?;
        match stream.write(&bytes[written..]) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    file: Option<&Path>,
    head: bool,
    deadline: Instant,
) -> io::Result<()> {
    let Some(path) = file else {
        let body = status.as_bytes();
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        write_by(stream, header.as_bytes(), deadline)?;
        write_by(stream, body, deadline)?;
        // A refused request may leave unread bytes, and closing over them resets the connection
        // before the client reads the answer: stop writing, and read what is left, within bounds.
        stream.shutdown(Shutdown::Write)?;
        let mut rest = [0_u8; 4096];
        let mut drained = 0_usize;
        while drained < MAX_HEAD * 8 {
            stream.set_read_timeout(Some(left(deadline)?.min(LINGER)))?;
            match stream.read(&mut rest) {
                Ok(0) | Err(_) => break,
                Ok(read) => drained += read,
            }
        }
        return Ok(());
    };
    let mut opened = File::open(path)?;
    let length = opened.metadata()?.len();
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {length}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        media_type(path)
    );
    write_by(stream, header.as_bytes(), deadline)?;
    if !head {
        let mut chunk = vec![0_u8; 64 << 10];
        loop {
            let read = opened.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            write_by(stream, &chunk[..read], deadline)?;
        }
    }
    stream.flush()
}
