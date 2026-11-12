//! The `ureq` stack against a loopback server: what a request carries and
//! answers, when an agent goes back to the pool, and what each `ureq` error
//! means to the caller.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_platform::http::{HeaderName, HeaderValue, HttpError, HttpRequest, Method};

use super::super::blocking::{Connection, Stack};
use super::{Agents, Stage, failure};

/// A request as the loopback server read it.
#[derive(Debug)]
struct Seen {
    head: String,
    body: Vec<u8>,
}

/// A keep-alive loopback server: answers each request on a connection with
/// `respond(path)`, until the client closes it.
struct Server {
    address: SocketAddr,
    accepted: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    fn start(respond: fn(&str) -> Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        let address = listener
            .local_addr()
            .expect("a bound listener has an address");
        let accepted = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (count, log) = (Arc::clone(&accepted), Arc::clone(&seen));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                count.fetch_add(1, Ordering::SeqCst);
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, respond, &log));
            }
        });
        Self {
            address,
            accepted,
            seen,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

fn serve(stream: TcpStream, respond: fn(&str) -> Vec<u8>, log: &Mutex<Vec<Seen>>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    loop {
        let mut head = String::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let end = line == "\r\n";
            head.push_str(&line);
            if end {
                break;
            }
        }
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .and_then(|length| length.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
        log.lock()
            .expect("BUG: nothing panics holding the log")
            .push(Seen { head, body });
        if writer.write_all(&respond(&path)).is_err() {
            return;
        }
    }
}

fn answer(path: &str) -> Vec<u8> {
    match path {
        // Headers, then a body that never ends.
        "/stream" => b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\npartial".to_vec(),
        "/missing" => {
            b"HTTP/1.1 404 Not Found\r\nX-Reply: none here\r\nContent-Length: 4\r\n\r\ngone"
                .to_vec()
        }
        _ => b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
    }
}

/// The body of `connection` read to its end.
fn drain(connection: &mut dyn Connection) -> Result<Vec<u8>, HttpError> {
    let mut body = Vec::new();
    while let Some(chunk) = connection.read()? {
        body.extend(chunk);
    }
    Ok(body)
}

/// An agent whose body ended goes back to the pool and serves the next
/// request on its connection; an abort that comes after that reaches no later
/// request, and an aborted agent never goes back.
///
/// Red-check: keep the agent in the abort's hands after it went back (the
/// late abort shuts the next request down); give an agent back whatever ended
/// its request (the aborted agent fails the next request); never give one
/// back (each request opens a connection of its own).
#[test]
fn an_agent_goes_back_to_the_pool_only_when_its_body_ended_unaborted() {
    let server = Server::start(answer);
    let stack = Agents::new();
    let get = |path: &str| HttpRequest::get(server.url(path));

    let mut first = stack.open(&get("/a")).expect("the request opens");
    let late = first.aborter();
    assert_eq!(first.send().expect("the head").status.as_u16(), 200);
    assert_eq!(drain(first.as_mut()).expect("the body"), b"abc");
    drop(first);
    late.abort();

    let mut second = stack.open(&get("/a")).expect("the request opens");
    assert_eq!(
        second.send().map(|head| head.status.as_u16()),
        Ok(200),
        "an abort after the body ended reaches no later request"
    );
    assert_eq!(drain(second.as_mut()).expect("the body"), b"abc");
    drop(second);
    assert_eq!(server.accepted(), 1, "the connection served both requests");

    let mut cut = stack.open(&get("/stream")).expect("the request opens");
    cut.send().expect("the head");
    assert_eq!(cut.read().expect("a chunk"), Some(b"partial".to_vec()));
    cut.aborter().abort();
    assert!(cut.read().is_err(), "the abort ends the blocked read");
    drop(cut);

    let mut after = stack.open(&get("/a")).expect("the request opens");
    assert_eq!(after.send().map(|head| head.status.as_u16()), Ok(200));
    assert_eq!(drain(after.as_mut()).expect("the body"), b"abc");
    assert_eq!(
        server.accepted(),
        2,
        "the aborted agent stayed out of the pool"
    );
}

/// A request carries its method, headers and body; any status is an answer,
/// with its headers; a URL that is not HTTP fails to open, and a server that
/// does not speak TLS fails the handshake.
///
/// Red-check: leave `http_status_as_error` on (the 404 is an error); drop the
/// request's body.
#[test]
fn a_request_carries_its_parts_and_any_status_answers() {
    let server = Server::start(answer);
    let stack = Agents::new();
    let request = HttpRequest::new(Method::Post, server.url("/missing"))
        .header(
            HeaderName::try_new("X-Name").expect("a token"),
            HeaderValue::try_new(b"value").expect("a value"),
        )
        .body(b"ping".to_vec());
    let mut connection = stack.open(&request).expect("the request opens");
    let head = connection.send().expect("a 404 is an answer");
    assert_eq!(head.status.as_u16(), 404);
    assert_eq!(
        head.headers.get("x-reply").and_then(|value| value.to_str()),
        Some("none here")
    );
    assert_eq!(drain(connection.as_mut()).expect("the body"), b"gone");
    {
        let seen = server
            .seen
            .lock()
            .expect("BUG: nothing panics holding the log");
        let [seen] = seen.as_slice() else {
            panic!("one request reached the server: {seen:?}");
        };
        assert!(
            seen.head.starts_with("POST /missing HTTP/1.1\r\n"),
            "{seen:?}"
        );
        assert!(seen.head.contains("x-name: value\r\n"), "{seen:?}");
        assert_eq!(seen.body, b"ping");
    }

    assert!(matches!(
        stack
            .open(&HttpRequest::get("ftp://example.test/file"))
            .map(|_| ()),
        Err(HttpError::InvalidUrl { .. })
    ));
    // A server that answers in plain HTTP as soon as it accepts.
    let plain = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
    let address = plain.local_addr().expect("a bound listener has an address");
    std::thread::spawn(move || {
        for stream in plain.incoming() {
            let Ok(mut stream) = stream else { return };
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            let mut rest = [0_u8; 512];
            while matches!(stream.read(&mut rest), Ok(read) if read > 0) {}
        }
    });
    let mut secure = stack
        .open(&HttpRequest::get(format!("https://{address}/a")))
        .expect("the request opens");
    let handshake = secure.send().map(|_| ());
    assert!(
        matches!(handshake, Err(HttpError::Tls { .. })),
        "a server that does not speak TLS fails the handshake: {handshake:?}"
    );
}

/// Each `ureq` error at each stage maps to the error the caller can act on,
/// a TLS failure inside an I/O error included.
///
/// Red-check: map every error by its stage alone (a timeout reads as a failed
/// connection, a certificate error as one too).
#[test]
fn ureq_errors_map_to_what_the_caller_can_act_on() {
    use std::io::{Error as Io, ErrorKind};

    use rustls::{CertificateError, Error as Tls};
    use ureq::{Error, Timeout};

    type Expected = fn(&HttpError) -> bool;

    let url = "https://example.test/";
    let expired = || Tls::InvalidCertificate(CertificateError::Expired);
    let cases: [(Error, Stage, Expected); 10] = [
        (Error::Timeout(Timeout::Global), Stage::Read, |error| {
            *error == HttpError::Timeout
        }),
        (Error::HostNotFound, Stage::Send, |error| {
            matches!(error, HttpError::Connect { .. })
        }),
        (Error::Rustls(expired()), Stage::Send, |error| {
            matches!(error, HttpError::Tls { .. })
        }),
        (Error::Io(Io::other(expired())), Stage::Send, |error| {
            matches!(error, HttpError::Tls { .. })
        }),
        (
            Error::Io(Io::new(ErrorKind::ConnectionRefused, "refused")),
            Stage::Send,
            |error| matches!(error, HttpError::Connect { .. }),
        ),
        (
            Error::Io(Io::new(ErrorKind::UnexpectedEof, "cut")),
            Stage::Read,
            |error| matches!(error, HttpError::Protocol { .. }),
        ),
        (
            Error::Io(Io::new(ErrorKind::TimedOut, "slow")),
            Stage::Read,
            |error| *error == HttpError::Timeout,
        ),
        (Error::TooManyRedirects, Stage::Send, |error| {
            matches!(error, HttpError::Protocol { .. })
        }),
        (
            Error::BadUri("no".to_owned()),
            Stage::Send,
            |error| matches!(error, HttpError::InvalidUrl { url } if url == "https://example.test/"),
        ),
        (Error::ConnectionFailed, Stage::Send, |error| {
            matches!(error, HttpError::Connect { .. })
        }),
    ];
    for (error, stage, expected) in cases {
        let mapped = failure(&error, stage, url);
        assert!(
            expected(&mapped),
            "{error:?} at {stage:?} became {mapped:?}"
        );
    }
}
