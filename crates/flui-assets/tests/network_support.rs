//! Bounded real HTTP responder shared by consumer contract tests.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Response {
    pub status: &'static str,
    pub body: &'static [u8],
    pub headers: &'static str,
    pub length: Option<usize>,
    pub delay: Duration,
}

impl Response {
    pub fn ok(body: &'static [u8]) -> Self {
        Self {
            status: "200 OK",
            body,
            headers: "",
            length: None,
            delay: Duration::ZERO,
        }
    }
}

pub fn spawn_server(responses: Vec<Response>, keep_alive: bool) -> (SocketAddr, JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server binds");
    let address = listener.local_addr().expect("server has an address");
    listener.set_nonblocking(true).expect("nonblocking accept");
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut connections = 0;
        let mut responses = responses.into_iter().peekable();
        while responses.peek().is_some() {
            assert!(
                Instant::now() < deadline,
                "server timed out waiting for requests"
            );
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("accept failed: {error}"),
            };
            connections += 1;
            // Accepted sockets can inherit nonblocking mode on Windows.
            stream
                .set_nonblocking(false)
                .expect("blocking request reads");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("write timeout");
            while responses.peek().is_some() && read_request(&mut stream) {
                let response = responses.next().expect("pending response");
                let connection = if keep_alive {
                    ""
                } else {
                    "Connection: close\r\n"
                };
                let headers = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\n{}{}\r\n",
                    response.status,
                    response.length.unwrap_or(response.body.len()),
                    response.headers,
                    connection,
                );
                stream
                    .write_all(headers.as_bytes())
                    .expect("response headers");
                std::thread::sleep(response.delay);
                let written = stream.write_all(response.body);
                if response.delay.is_zero() {
                    written.expect("response body");
                }
                if !keep_alive {
                    break;
                }
            }
        }
        connections
    });
    (address, server)
}

fn read_request(stream: &mut TcpStream) -> bool {
    let mut headers = Vec::new();
    loop {
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {
                assert!(headers.is_empty(), "incomplete request");
                return false;
            }
            Ok(_) => headers.push(byte[0]),
            Err(error) => panic!("request read failed: {error}"),
        }
        assert!(headers.len() < 16_384, "request headers too large");
        if headers.ends_with(b"\r\n\r\n") {
            return true;
        }
    }
}
